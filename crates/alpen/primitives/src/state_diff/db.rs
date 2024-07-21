use std::collections::HashMap;

use reth_codecs::Compact;
use reth_db_api::{
    table::{Compress, Decompress},
    DatabaseError,
};
use reth_primitives::{Address, Bytecode, B256, U256};
use revm::{
    db::{states::StorageSlot, AccountStatus, BundleAccount, StorageWithOriginalValues},
    primitives::AccountInfo,
};
use serde::{Deserialize, Serialize};

use crate::state_diff::StateDiff;

// BundleAccount, etc cannot be saved to db as-is due to missing traits and HashMap
// This provides a representation that is easier to serialize to db

/// represents the state diff for a single block storable in db
#[derive(Clone, Debug, PartialEq, Eq, Compact, Default, Serialize, Deserialize)]
pub struct DbStateDiff {
    /// Account state.
    pub account_diffs: Vec<DbBundleAccount>,
}

impl DbStateDiff {
    /// create new instance
    pub fn new(account_diffs: Vec<DbBundleAccount>) -> Self {
        Self { account_diffs }
    }
}

impl Into<DbStateDiff> for StateDiff {
    fn into(self) -> DbStateDiff {
        let account_diffs = self
            .state
            .into_iter()
            .map(|(address, bundle_account)| DbBundleAccount {
                address,
                prev_info: bundle_account.original_info.map(Into::into),
                info: bundle_account.info.map(|info| {
                    let code = self.contracts.get(&info.code_hash).cloned();
                    DbAccountInfo::from_account_info(info, code)
                }),
                storage: bundle_account.storage.into(),
                status: from_account_status(bundle_account.status),
            })
            .collect();

        DbStateDiff { account_diffs }
    }
}

impl Into<StateDiff> for DbStateDiff {
    fn into(self) -> StateDiff {
        let contracts = HashMap::<B256, Bytecode>::default();
        let state = self
            .account_diffs
            .into_iter()
            .map(|account_diff| {
                let bundle_account = BundleAccount {
                    info: account_diff.info.map(Into::into),
                    original_info: account_diff.prev_info.map(Into::into),
                    status: to_account_status(account_diff.status),
                    storage: account_diff.storage.into(),
                };

                (account_diff.address, bundle_account)
            })
            .collect();

        StateDiff { state, contracts }
    }
}

/// equivalent to `revm::db::BundleAcocunt`
#[derive(Clone, Debug, PartialEq, Eq, Default, Compact, Serialize, Deserialize)]
pub struct DbBundleAccount {
    /// Account address
    pub address: Address,
    /// Previous account info
    pub prev_info: Option<DbAccountInfo>,
    /// current account info
    pub info: Option<DbAccountInfo>,
    /// status
    pub status: u64,
    /// storage diff
    pub storage: DbStorageDiff,
}

/// equivalent to `revm::primitives::state::AccountInfo`
#[derive(Clone, Debug, PartialEq, Eq, Default, Compact, Serialize, Deserialize)]
pub struct DbAccountInfo {
    /// Account balance.
    pub balance: U256,
    /// Account nonce.
    pub nonce: u64,
    /// code hash,
    pub code_hash: B256,
    /// code: Some() new contract created
    pub code: Option<Bytecode>,
}

impl Into<DbAccountInfo> for AccountInfo {
    fn into(self) -> DbAccountInfo {
        DbAccountInfo {
            balance: self.balance,
            nonce: self.nonce,
            code_hash: self.code_hash,
            code: self.code.map(Bytecode),
        }
    }
}

impl Into<AccountInfo> for DbAccountInfo {
    fn into(self) -> AccountInfo {
        AccountInfo {
            balance: self.balance,
            nonce: self.nonce,
            code_hash: self.code_hash,
            code: self.code.map(|code| code.0),
        }
    }
}

impl DbAccountInfo {
    /// create from AccountInfo and replace code
    pub fn from_account_info(account_info: AccountInfo, code: Option<Bytecode>) -> Self {
        let mut info: Self = account_info.into();
        info.code = info.code.or(code);
        info
    }
}

/// all storage diffs for a block
#[derive(Clone, Debug, PartialEq, Eq, Default, Compact, Serialize, Deserialize)]
pub struct DbStorageDiff(Vec<DbStorageDiffEntry>);

impl DbStorageDiff {
    /// new storage diff
    pub fn new(diff_entries: Vec<DbStorageDiffEntry>) -> Self {
        Self(diff_entries)
    }
}

impl Into<DbStorageDiff> for StorageWithOriginalValues {
    fn into(self) -> DbStorageDiff {
        let diff_entries = self
            .into_iter()
            .map(|(slot_address, entry)| {
                DbStorageDiffEntry::new(
                    slot_address,
                    entry.previous_or_original_value,
                    entry.present_value,
                )
            })
            .collect();

        DbStorageDiff(diff_entries)
    }
}

impl Into<StorageWithOriginalValues> for DbStorageDiff {
    fn into(self) -> StorageWithOriginalValues {
        self.0
            .into_iter()
            .map(|entry| {
                (entry.address, StorageSlot::new_changed(entry.previous_value, entry.current_value))
            })
            .collect()
    }
}

/// diff for a single storage entry
#[derive(Clone, Debug, PartialEq, Eq, Default, Compact, Serialize, Deserialize)]
pub struct DbStorageDiffEntry {
    address: U256,
    previous_value: U256,
    current_value: U256,
}

impl DbStorageDiffEntry {
    /// create new `StorageDiffEntry`
    pub fn new(address: U256, previous_value: U256, current_value: U256) -> Self {
        Self { address, previous_value, current_value }
    }
}

// FIXME: add custom enum to known types supported by Compact
fn from_account_status(status: AccountStatus) -> u64 {
    match status {
        AccountStatus::LoadedNotExisting => 0,
        AccountStatus::Loaded => 1,
        AccountStatus::LoadedEmptyEIP161 => 2,
        AccountStatus::InMemoryChange => 3,
        AccountStatus::Changed => 4,
        AccountStatus::Destroyed => 5,
        AccountStatus::DestroyedChanged => 6,
        AccountStatus::DestroyedAgain => 7,
    }
}

fn to_account_status(status: u64) -> AccountStatus {
    match status {
        0 => AccountStatus::LoadedNotExisting,
        1 => AccountStatus::Loaded,
        2 => AccountStatus::LoadedEmptyEIP161,
        3 => AccountStatus::InMemoryChange,
        4 => AccountStatus::Changed,
        5 => AccountStatus::Destroyed,
        6 => AccountStatus::DestroyedChanged,
        7 => AccountStatus::DestroyedAgain,
        _ => panic!("invalid status"),
    }
}


// db related custom trait impls

impl Compress for DbStateDiff {
    type Compressed = Vec<u8>;
    fn compress_to_buf<B: bytes::BufMut + AsMut<[u8]>>(self, buf: &mut B) {
        let _ = Compact::to_compact(self, buf);
    }
}

impl Decompress for DbStateDiff {
    fn decompress<B: AsRef<[u8]>>(value: B) -> Result<DbStateDiff, DatabaseError> {
        let value = value.as_ref();
        let (obj, _) = Compact::from_compact(&value, value.len());
        Ok(obj)
    }
}
