use std::collections::BTreeMap;
use std::ffi::OsString;

use async_trait::async_trait;
use fedimint_core::cancellable::Cancellable;
use fedimint_core::config::{
    ConfigGenParams, DkgPeerMsg, ModuleConfigResponse, ModuleGenParams, ServerModuleConfig,
    TypedServerModuleConfig, TypedServerModuleConsensusConfig,
};
use fedimint_core::core::{ModuleInstanceId, ModuleKind};
use fedimint_core::db::{Database, DatabaseTransaction, DatabaseVersion};
use fedimint_core::encoding::Encodable;
use fedimint_core::module::ModuleGen;
use fedimint_core::module::__reexports::serde_json;
use fedimint_core::net::peers::MuxPeerConnections;
use fedimint_core::server::DynServerModule;
use fedimint_core::task::TaskGroup;
use fedimint_core::{NumPeers, PeerId};
use fedimint_core::module::CoreConsensusVersion;
use fedimint_core::module::ModuleConsensusVersion;
use serde::{Deserialize, Serialize};

use crate::common::PoolDecoder;
use crate::config::{
    EpochConfig, OracleConfig, PoolConfig, PoolConfigClient, PoolConfigConsensus, PoolConfigPrivate,
};
use crate::stability_core::CollateralRatio;
use crate::{StabilityPool, KIND};

// The default global max feerate.
// TODO: Have this actually in config.
pub const DEFAULT_GLOBAL_MAX_FEERATE: u64 = 100_000;

/// The default epoch length is 24hrs (represented in seconds).
// pub const DEFAULT_EPOCH_LENGTH: u64 = 24 * 60 * 60;
pub const DEFAULT_EPOCH_LENGTH: u64 = 40; // TODO: This is just for testing

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfigGenParams {
    pub important_param: u64,
    #[serde(default)]
    pub start_epoch_at: Option<time::PrimitiveDateTime>,
    /// this is in seconds
    pub epoch_length: u64,
    pub oracle_config: OracleConfig,
    /// The ratio of seeker position to provider collateral
    #[serde(default)]
    pub collateral_ratio: CollateralRatio,
}

impl ModuleGenParams for PoolConfigGenParams {
    const MODULE_NAME: &'static str = "stabilitypool";
}

impl Default for PoolConfigGenParams {
    fn default() -> Self {
        Self {
            important_param: 3,
            start_epoch_at: None,
            epoch_length: DEFAULT_EPOCH_LENGTH,
            oracle_config: OracleConfig::default(),
            collateral_ratio: Default::default(),
        }
    }
}

#[derive(Debug)]
pub struct PoolConfigGenerator;

#[async_trait]
impl ModuleGen for PoolConfigGenerator {
    const KIND: ModuleKind = KIND;
    const DATABASE_VERSION: DatabaseVersion = DatabaseVersion(1);
    type Decoder = PoolDecoder;

    fn decoder(&self) -> PoolDecoder {
        PoolDecoder
    }

    fn versions(&self, _core: CoreConsensusVersion) -> &[ModuleConsensusVersion] {
        &[ModuleConsensusVersion(0)]
    }

    async fn init(
        &self,
        cfg: ServerModuleConfig,
        _db: Database,
        _env: &BTreeMap<OsString, OsString>,
        _task_group: &mut TaskGroup,
    ) -> anyhow::Result<DynServerModule> {
        Ok(StabilityPool::new(cfg.to_typed()?).into())
    }

    fn trusted_dealer_gen(
        &self,
        peers: &[PeerId],
        params: &ConfigGenParams,
    ) -> BTreeMap<PeerId, ServerModuleConfig> {
        let params = params
            .get::<PoolConfigGenParams>()
            .expect("Invalid mint params");

        let mint_cfg: BTreeMap<_, PoolConfig> = peers
            .iter()
            .map(|&peer| {
                let config = PoolConfig {
                    private: PoolConfigPrivate { peer_id: peer },
                    consensus: PoolConfigConsensus {
                        epoch: EpochConfig {
                            start_epoch_at: params
                                .start_epoch_at
                                .map(|prim_datetime| prim_datetime.assume_utc())
                                .unwrap_or_else(|| time::OffsetDateTime::now_utc())
                                .unix_timestamp() as _,
                            epoch_length: params.epoch_length,
                            price_threshold: peers.threshold() as _,
                            max_feerate_ppm: DEFAULT_GLOBAL_MAX_FEERATE,
                            collateral_ratio: params.collateral_ratio,
                        },
                        oracle: params.oracle_config.clone(),
                    },
                };
                (peer, config)
            })
            .collect();

        mint_cfg
            .into_iter()
            .map(|(k, v)| (k, v.to_erased()))
            .collect()
    }

    async fn distributed_gen(
        &self,
        _connections: &MuxPeerConnections<ModuleInstanceId, DkgPeerMsg>,
        our_id: &PeerId,
        _instance_id: ModuleInstanceId,
        peers: &[PeerId],
        params: &ConfigGenParams,
        _task_group: &mut TaskGroup,
    ) -> anyhow::Result<Cancellable<ServerModuleConfig>> {
        let params = params
            .get::<PoolConfigGenParams>()
            .expect("Invalid mint params");

        let server = PoolConfig {
            private: PoolConfigPrivate { peer_id: *our_id },
            consensus: PoolConfigConsensus {
                epoch: EpochConfig {
                    start_epoch_at: params
                        .start_epoch_at
                        .map(|prim_datetime| prim_datetime.assume_utc())
                        .unwrap_or_else(|| time::OffsetDateTime::now_utc())
                        .unix_timestamp() as _,
                    epoch_length: params.epoch_length,
                    price_threshold: peers.threshold() as _,
                    max_feerate_ppm: DEFAULT_GLOBAL_MAX_FEERATE,
                    collateral_ratio: params.collateral_ratio,
                },
                oracle: params.oracle_config,
            },
        };

        Ok(Ok(server.to_erased()))
    }

    fn to_config_response(
        &self,
        config: serde_json::Value,
    ) -> anyhow::Result<fedimint_core::config::ModuleConfigResponse> {
        let config = serde_json::from_value::<PoolConfigConsensus>(config)?;

        Ok(ModuleConfigResponse {
            client: config.to_client_config(),
            consensus_hash: config.consensus_hash()?,
        })
    }

    fn hash_client_module(
        &self,
        config: serde_json::Value,
    ) -> anyhow::Result<bitcoin::hashes::sha256::Hash> {
        serde_json::from_value::<PoolConfigClient>(config)?.consensus_hash()
    }

    fn validate_config(&self, identity: &PeerId, config: ServerModuleConfig) -> anyhow::Result<()> {
        config.to_typed::<PoolConfig>()?.validate_config(identity)
    }

    async fn dump_database(
        &self,
        _dbtx: &mut DatabaseTransaction<'_>,
        _prefix_names: Vec<String>,
    ) -> Box<dyn Iterator<Item = (String, Box<dyn erased_serde::Serialize + Send>)> + '_> {
        Box::new(BTreeMap::new().into_iter())
    }
}
