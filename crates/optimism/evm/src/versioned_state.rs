use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};
use revm_primitives::{AccountInfo, Address, Bytecode};
use revm_primitives::db::Database;
use reth_primitives::{B256, U256};
use crate::api::{StateReader, StateResult, UpdatableState};
use crate::TxIndex;
use crate::versioned_storage::VersionedStorage;


const READ_ERR: &str = "Error: read value missing in the versioned storage";

pub type StorageEntry = (Address, U256);

#[cfg_attr(any(feature = "testing", test), derive(Clone))]
#[derive(Debug, Default, PartialEq, Eq)]
pub struct StateMaps {
    pub nonces: HashMap<Address, U256>,
    pub storage: HashMap<StorageEntry, U256>,
}

#[derive(Debug)]
pub struct VersionedState<S: StateReader> {
    // TODO add more
    initial_state: S,
    storage: VersionedStorage<(Address, U256), U256>,
    nonces: VersionedStorage<Address, U256>,
}

impl<S: StateReader> VersionedState<S> {
    pub fn new(initial_state: S) -> Self {
        VersionedState {
            initial_state,
            storage: VersionedStorage::default(),
            nonces: VersionedStorage::default(),
        }
    }

    fn get_writes_up_to_index(&mut self, tx_index: TxIndex) -> StateMaps {
        StateMaps {
            storage: self.storage.get_writes_up_to_index(tx_index),
            nonces: self.nonces.get_writes_up_to_index(tx_index),
        }
    }

    #[cfg(any(feature = "testing", test))]
    pub fn get_writes_of_index(&self, tx_index: TxIndex) -> StateMaps {
        StateMaps {
            storage: self.storage.get_writes_of_index(tx_index),
            nonces: self.nonces.get_writes_of_index(tx_index),
        }
    }

    fn validate_reads(&mut self, tx_index: TxIndex, reads: &StateMaps) -> bool {
        // If is the first transaction in the chunk, then the read set is valid. Since it has no
        // predecessors, there's nothing to compare it to.
        if tx_index == 0 {
            return true;
        }
        // Ignore values written by the current transaction.
        let tx_index = tx_index - 1;
        for (&(contract_address, storage_key), expected_value) in &reads.storage {
            let value =
                self.storage.read(tx_index, (contract_address, storage_key)).expect(READ_ERR);

            if &value != expected_value {
                return false;
            }
        }

        for (&contract_address, expected_value) in &reads.nonces {
            let value = self.nonces.read(tx_index, contract_address).expect(READ_ERR);

            if &value != expected_value {
                return false;
            }
        }

        // All values in the read set match the values from versioned state, return true.
        true
    }

    fn apply_writes(
        &mut self,
        tx_index: TxIndex,
        writes: &StateMaps,
    ) {
        for (&key, &value) in &writes.storage {
            self.storage.write(tx_index, key, value);
        }
        for (&key, &value) in &writes.nonces {
            self.nonces.write(tx_index, key, value);
        }
    }

    fn delete_writes(
        &mut self,
        tx_index: TxIndex,
        writes: &StateMaps,
    ) {
        for &key in writes.storage.keys() {
            self.storage.delete_write(key, tx_index);
        }
        for &key in writes.nonces.keys() {
            self.nonces.delete_write(key, tx_index);
        }
    }

    fn into_initial_state(self) -> S {
        self.initial_state
    }
}

impl<U: UpdatableState> VersionedState<U> {
    pub fn commit_and_recover_block_state(
        mut self,
        n_committed_txs: usize,
    ) -> U {
        if n_committed_txs == 0 {
            return self.into_initial_state();
        }
        let commit_index = n_committed_txs - 1;
        let writes = self.get_writes_up_to_index(commit_index);

        let mut state = self.into_initial_state();
        state.apply_writes(&writes); // todo add cache state to commit
        state
    }
}

pub struct VersionedStateProxy<S: StateReader> {
    pub tx_index: TxIndex,
    pub state: Arc<Mutex<VersionedState<S>>>,
}

pub struct ThreadSafeVersionedState<S: StateReader>(Arc<Mutex<VersionedState<S>>>);

pub type LockedVersionedState<'a, S> = MutexGuard<'a, VersionedState<S>>;

impl<S: StateReader> Database for VersionedStateProxy<S> {
    type Error = ();

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        todo!()
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        todo!()
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        todo!()
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        todo!()
    }
}

impl<S: StateReader> ThreadSafeVersionedState<S> {
    pub fn new(versioned_state: VersionedState<S>) -> Self {
        ThreadSafeVersionedState(Arc::new(Mutex::new(versioned_state)))
    }

    pub fn pin_version(&self, tx_index: TxIndex) -> VersionedStateProxy<S> {
        VersionedStateProxy { tx_index, state: self.0.clone() }
    }

    pub fn into_inner_state(self) -> VersionedState<S> {
        Arc::try_unwrap(self.0)
            .unwrap_or_else(|_| {
                panic!(
                    "To consume the versioned state, you must have only one strong reference to \
                     self. Consider dropping objects that hold a reference to it."
                )
            })
            .into_inner()
            .expect("No other mutex should hold the versioned state while calling this method.")
    }
}

impl<S: StateReader> VersionedStateProxy<S> {
    fn state(&self) -> LockedVersionedState<'_, S> {
        self.state.lock().expect("Failed to acquire state lock.")
    }

    pub fn validate_reads(&self, reads: &StateMaps) -> bool {
        self.state().validate_reads(self.tx_index, reads)
    }

    pub fn delete_writes(&self, writes: &StateMaps) {
        self.state().delete_writes(self.tx_index, writes);
    }
}

impl<S: StateReader> StateReader for VersionedStateProxy<S> {
    fn get_storage_at(&self, contract_address: Address, key: U256) -> StateResult<U256> {
        let mut state = self.state();
        match state.storage.read(self.tx_index, (contract_address, key)) {
            Some(value) => Ok(value),
            None => {
                let initial_value = state.initial_state.get_storage_at(contract_address, key)?;
                state.storage.set_initial_value((contract_address, key), initial_value);
                Ok(initial_value)
            }
        }
    }

    fn get_nonce_at(&self, contract_address: Address) -> StateResult<U256> {
        let mut state = self.state();
        match state.nonces.read(self.tx_index, contract_address) {
            Some(value) => Ok(value),
            None => {
                let initial_value = state.initial_state.get_nonce_at(contract_address)?;
                state.nonces.set_initial_value(contract_address, initial_value);
                Ok(initial_value)
            }
        }
    }
}

impl<S: StateReader> UpdatableState for VersionedStateProxy<S> {
    fn apply_writes(
        &mut self,
        writes: &StateMaps,
    ) {
        self.state().apply_writes(self.tx_index, writes)
    }
}

impl StateMaps {
    pub fn extend(&mut self, other: &Self) {
        self.nonces.extend(&other.nonces);
        self.storage.extend(&other.storage);
    }

    pub fn diff(&self, other: &Self) -> Self {
        Self {
            nonces: strict_subtract_mappings(&self.nonces, &other.nonces),
            storage: strict_subtract_mappings(&self.storage, &other.storage),
        }
    }
}

pub fn strict_subtract_mappings<K, V>(
    source: &HashMap<K, V>,
    subtract: &HashMap<K, V>,
) -> HashMap<K, V>
where
    K: Clone + Eq + std::hash::Hash,
    V: Clone + PartialEq,
{
    source
        .iter()
        .filter(|(k, v)| subtract.get(k).expect(STRICT_SUBTRACT_MAPPING_ERROR) != *v)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

pub const STRICT_SUBTRACT_MAPPING_ERROR: &str =
    "The source mapping keys are not a subset of the subtract mapping keys";