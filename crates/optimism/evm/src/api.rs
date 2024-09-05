use std::collections::{HashMap, HashSet};
use revm_primitives::{AccountInfo, Address, Bytecode};
use revm_primitives::db::Database;
use thiserror::Error;
use reth_primitives::{B256, U256};
use crate::versioned_state::StateMaps;

pub type StateResult<T> = Result<T, StateError>;
pub trait StateReader {
    /// Returns the storage value under the given key in the given contract instance (represented by
    /// its address).
    /// Default: 0 for an uninitialized contract address.
    fn get_storage_at(
        &self,
        contract_address: Address,
        key: U256,
    ) -> StateResult<U256>;

    /// Default: 0 for an uninitialized contract address.
    fn get_nonce_at(&self, contract_address: Address) -> StateResult<U256>;

}


pub trait UpdatableState: StateReader {
    fn apply_writes(
        &mut self,
        writes: &StateMaps,
    );
}


#[derive(Debug, Error)]
pub enum StateError {
    #[error("ERROR IN: {0}.")]
    NormalError(String),
    #[error("Cannot deploy contract at address 0.")]
    OutOfRangeContractAddress,
    #[error("Requested {0:?} is unavailable for deployment.")]
    UnavailableContractAddress(Address),
    #[error("Failed to read from state: {0}.")]
    StateReadError(String),
}
