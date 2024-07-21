//! state diff data structures


/// db related primitives
pub mod db;

use std::collections::HashMap;

use reth_primitives::{Address, Bytecode, B256};
use revm::db::{BundleAccount, BundleState};

/// represents the state diff for a single block
/// subset of `revm::db::BundleState`
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct StateDiff {
    /// Account state diffs
    pub state: HashMap<Address, BundleAccount>,
    /// new contracts
    pub contracts: HashMap<B256, Bytecode>,
}

impl Into<StateDiff> for BundleState {
    fn into(self) -> StateDiff {
        let BundleState { state, contracts, .. } = self;
        StateDiff {
            state,
            contracts: contracts
                .into_iter()
                .map(|(code_hash, bytes)| (code_hash, Bytecode(bytes)))
                .collect(),
        }
    }
}


/// represent async db operations (insert, delete) for state diffs
#[derive(Debug)]
pub enum StateDiffOp {
    /// insert
    Put(B256, StateDiff),
    /// delete
    Del(B256),
}