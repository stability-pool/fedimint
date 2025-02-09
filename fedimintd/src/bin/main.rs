use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use fedimint_core::db::Database;
use fedimint_core::task::{sleep, TaskGroup};
use fedimint_server::config::io::{read_server_config, DB_FILE, JSON_EXT, LOCAL_CONFIG};
use fedimint_server::consensus::FedimintConsensus;
use fedimint_server::logging::TracingSetup;
use fedimint_server::FedimintServer;
use fedimintd::ui::{run_ui, UiMessage};
use fedimintd::*;
use futures::FutureExt;
use stabilitypool::PoolConfigGenerator;
use tokio::select;
use tracing::{debug, error, info, warn};

/// Time we will wait before forcefully shutting down tasks
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Parser)]
pub struct ServerOpts {
    /// Path to folder containing federation config files
    pub data_dir: PathBuf,
    /// Password to encrypt sensitive config files
    #[arg(env = "FM_PASSWORD")]
    pub password: String,
    /// Port to run admin UI on
    #[arg(long = "listen-ui", env = "FM_LISTEN_UI")]
    pub listen_ui: Option<SocketAddr>,
    #[arg(long = "tokio-console-bind", env = "FM_TOKIO_CONSOLE_BIND")]
    pub tokio_console_bind: Option<SocketAddr>,
    #[arg(long, default_value = "false")]
    pub with_telemetry: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args();
    if let Some(ref arg) = args.nth(1) {
        if arg.as_str() == "version-hash" {
            println!("{CODE_VERSION}");
            return Ok(());
        }
    }

    info!("Starting fedimintd (version: {CODE_VERSION})");

    let opts: ServerOpts = ServerOpts::parse();
    TracingSetup::default()
        .tokio_console_bind(opts.tokio_console_bind)
        .with_jaeger(opts.with_telemetry)
        .init()?;

    let mut root_task_group = TaskGroup::new();
    root_task_group.install_kill_handler();

    // DO NOT REMOVE, or spawn_local tasks won't run anymore
    let local_task_set = tokio::task::LocalSet::new();
    let _guard = local_task_set.enter();

    let task_group = root_task_group.clone();
    root_task_group
        .spawn_local("main", move |_task_handle| async move {
            match run(opts, task_group.clone()).await {
                Ok(()) => {}
                Err(e) => {
                    error!(?e, "Main task returned error, shutting down");
                    task_group.shutdown().await;
                }
            }
        })
        .await;

    let shutdown_future = root_task_group
        .make_handle()
        .make_shutdown_rx()
        .await
        .then(|_| async {
            let shutdown_seconds = SHUTDOWN_TIMEOUT.as_secs();
            info!("Shutdown called, waiting {shutdown_seconds}s for main task to finish");
            sleep(SHUTDOWN_TIMEOUT).await;
        });

    select! {
        _ = shutdown_future => {
            debug!("Terminating main task");
        }
        _ = local_task_set => {
            warn!("local_task_set finished before shutdown was called");
        }
    }

    if let Err(err) = root_task_group.join_all(Some(SHUTDOWN_TIMEOUT)).await {
        error!(?err, "Error while shutting down task group");
    }

    info!("Shutdown complete");

    #[cfg(feature = "telemetry")]
    opentelemetry::global::shutdown_tracer_provider();

    // Should we ever shut down without an error code?
    std::process::exit(-1);
}

async fn run(opts: ServerOpts, mut task_group: TaskGroup) -> anyhow::Result<()> {
    let (ui_sender, mut ui_receiver) = tokio::sync::mpsc::channel(1);

    info!("Starting pre-check");

    // Run admin UI if a socket address was given for it
    if let Some(listen_ui) = opts.listen_ui {
        // Spawn admin UI
        let data_dir = opts.data_dir.clone();
        let ui_task_group = task_group.make_subgroup().await;
        let password = opts.password.clone();
        task_group
            .spawn("admin-ui", move |_| async move {
                run_ui(
                    data_dir,
                    ui_sender,
                    listen_ui,
                    password,
                    ui_task_group,
                    module_registry(),
                )
                .await;
            })
            .await;

        // If federation configs (e.g. local.json) missing, wait for admin UI to report
        // DKG completion
        let local_cfg_path = opts.data_dir.join(LOCAL_CONFIG).with_extension(JSON_EXT);
        if !std::path::Path::new(&local_cfg_path).exists() {
            loop {
                if let UiMessage::DkgSuccess = ui_receiver
                    .recv()
                    .await
                    .expect("failed to receive setup message")
                {
                    break;
                }
            }
        }
    }

    info!("Starting consensus");

    let cfg = read_server_config(&opts.password, opts.data_dir.clone())?;

    let decoders = module_registry().decoders(cfg.iter_module_instances())?;

    let db = Database::new(
        fedimint_rocksdb::RocksDb::open(opts.data_dir.join(DB_FILE))?,
        decoders.clone(),
    );

    let (consensus, tx_receiver) =
        FedimintConsensus::new(cfg.clone(), db, module_registry(), &mut task_group).await?;

    FedimintServer::run(cfg, consensus, tx_receiver, decoders, &mut task_group).await?;

    Ok(())
}
