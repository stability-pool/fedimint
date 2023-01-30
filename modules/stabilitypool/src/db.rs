use std::fmt::Debug;

use fedimint_core::db::{DatabaseKey, DatabaseRecord, DatabaseTransaction};
use fedimint_core::encoding::{Decodable, Encodable};
use fedimint_core::impl_db_prefix_const;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use strum_macros::EnumIter;

use crate::action::ActionStaged;
use crate::epoch::EpochOutcome;
use crate::{AccountBalance, EpochEnd};

#[repr(u8)]
#[derive(Clone, EnumIter, Debug)]
pub enum DbKeyPrefix {
    /// Account entry prefix.
    ///   Key: x-only-pubkey (represents the account)
    /// Value: account balances + pool side
    Account = 0xE0,

    /// Successful deposit outcome entry prefix.
    ///   Key: tx outpoint
    /// Value: x-only-pubkey (represents the account where funds are deposited)
    DepositOutcome,

    /// Where we store epoch outcome.
    ///   Key: epoch_id
    /// Value: EpochOutcome
    EpochOutcome,

    /// Epoch consensus state information.
    ///   Key: ~,
    /// Value: epoch_id
    LastEpochEnded,
    LastEpochSettled,

    /// The last valid `epoch_end` item we got from given peer (Consensus Item).
    ///   Key: PeerId
    /// Value: EpochEnd
    EpochEnd,

    /// User action staged for the next epoch (Consensus Item)
    ///   Key: x-only-pubkey (account id)
    /// Value: action::ActionStaged
    ActionStaged,
}

impl std::fmt::Display for DbKeyPrefix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, Encodable, Decodable, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct AccountBalanceKey(pub secp256k1_zkp::XOnlyPublicKey);

#[derive(
    Debug, Clone, Encodable, Decodable, Eq, PartialEq, Hash, Serialize, Deserialize, Ord, PartialOrd,
)]
pub struct AccountBalanceKeyPrefix;

impl_db_prefix_const!(
    key = AccountBalanceKey,
    value = AccountBalance,
    db_prefix = DbKeyPrefix::Account,
    query_prefix = AccountBalanceKeyPrefix
);

#[derive(Debug, Clone, Encodable, Decodable, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct DepositOutcomeKey(pub fedimint_core::OutPoint);

#[derive(
    Debug, Clone, Encodable, Decodable, Eq, PartialEq, Hash, Serialize, Deserialize, Ord, PartialOrd,
)]
pub struct DepositOutcomePrefix;

impl_db_prefix_const!(
    key = DepositOutcomeKey,
    value = secp256k1_zkp::XOnlyPublicKey,
    db_prefix = DbKeyPrefix::DepositOutcome,
    query_prefix = DepositOutcomePrefix
);

#[derive(
    Debug, Clone, Encodable, Decodable, Eq, PartialEq, Hash, Serialize, Deserialize, Ord, PartialOrd,
)]
pub struct EpochOutcomeKey(pub u64);

#[derive(Debug, Clone, Encodable, Decodable, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct EpochOutcomeKeyPrefix;

impl_db_prefix_const!(
    key = EpochOutcomeKey,
    value = EpochOutcome,
    db_prefix = DbKeyPrefix::EpochOutcome,
    query_prefix = EpochOutcomeKeyPrefix
);

#[derive(Debug, Clone, Encodable, Decodable, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct LastEpochSettledKey;

#[derive(
    Debug, Clone, Encodable, Decodable, Eq, PartialEq, Hash, Serialize, Deserialize, Ord, PartialOrd,
)]
pub struct LastEpochSettledPrefix;

impl_db_prefix_const!(
    key = LastEpochSettledKey,
    value = u64,
    db_prefix = DbKeyPrefix::LastEpochSettled,
    query_prefix = LastEpochSettledPrefix
);

#[derive(Debug, Clone, Encodable, Decodable, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct LastEpochEndedKey;

#[derive(
    Debug, Clone, Encodable, Decodable, Eq, PartialEq, Hash, Serialize, Deserialize, Ord, PartialOrd,
)]
pub struct LastEpochEndedPrefix;

impl_db_prefix_const!(
    key = LastEpochEndedKey,
    value = u64,
    db_prefix = DbKeyPrefix::LastEpochEnded,
    query_prefix = LastEpochEndedPrefix
);

#[derive(
    Debug, Clone, Encodable, Decodable, Eq, PartialEq, Hash, Serialize, Deserialize, Ord, PartialOrd,
)]
pub struct EpochEndKey(pub fedimint_core::PeerId);

#[derive(
    Debug, Clone, Encodable, Decodable, Eq, PartialEq, Hash, Serialize, Deserialize, Ord, PartialOrd,
)]
pub struct EpochEndKeyPrefix;

impl_db_prefix_const!(
    key = EpochEndKey,
    value = EpochEnd,
    db_prefix = DbKeyPrefix::EpochEnd,
    query_prefix = EpochEndKeyPrefix,
);

#[derive(
    Debug, Clone, Encodable, Decodable, Eq, PartialEq, Hash, Serialize, Deserialize, Ord, PartialOrd,
)]
pub struct ActionStagedKey(pub secp256k1_zkp::XOnlyPublicKey);

#[derive(
    Debug, Clone, Encodable, Decodable, Eq, PartialEq, Hash, Serialize, Deserialize, Ord, PartialOrd,
)]
pub struct ActionStagedKeyPrefix;

impl_db_prefix_const!(
    key = ActionStagedKey,
    value = ActionStaged,
    db_prefix = DbKeyPrefix::ActionStaged,
    query_prefix = ActionStagedKeyPrefix
);

pub async fn get<K>(dbtx: &mut DatabaseTransaction<'_>, key: &K) -> Option<K::Value>
where
    K: DatabaseKey + DatabaseRecord,
{
    dbtx.get_value(key).await.expect("db error")
}

pub async fn set<K, V>(dbtx: &mut DatabaseTransaction<'_>, key: &K, value: &V) -> Option<V>
where
    K: Encodable + Decodable + Debug + DatabaseRecord<Value = V>,
{
    dbtx.insert_entry(key, value).await.expect("db error")
}

pub async fn pop<K, V>(dbtx: &mut DatabaseTransaction<'_>, key: &K) -> Option<V>
where
    K: Encodable + Decodable + Debug + DatabaseRecord<Value = V>,
{
    dbtx.remove_entry(key).await.expect("db error")
}

pub async fn prefix_remove_all<'a, P>(dbtx: &mut DatabaseTransaction<'_>, key_prefix: &'a P)
where
    P: Encodable + Debug + DatabaseRecord + Decodable,
{
    dbtx.remove_by_prefix(key_prefix).await.expect("db error")
}

// BROKEN!
pub async fn prefix_values<'a, KP: 'a>(
    dbtx: &'a mut DatabaseTransaction<'_>,
    key_prefix: &'a KP,
) -> impl Iterator<Item = KP::Value> + 'a
where
    KP: DatabaseRecord + Encodable + Decodable,
{
    dbtx.find_by_prefix(key_prefix)
        .await
        .map(|res| res.expect("db error").1)
        .collect::<Vec<_>>()
        .await
        .into_iter()
}
