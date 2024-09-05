use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use reth_primitives::{Address, U256};
use crate::api::{StateReader, StateResult, UpdatableState};
use crate::versioned_state::StateMaps;

#[derive(Debug)]
pub struct CachedState<S: StateReader> {
    pub state: S,
    // Invariant: read/write access is managed by CachedState.
    // Using interior mutability to update caches during `State`'s immutable getters.
    pub(crate) cache: RefCell<StateCache>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct StateCache {
    // Reader's cached information; initial values, read before any write operation (per cell).
    pub(crate) initial_reads: StateMaps,

    // Writer's cached information.
    pub(crate) writes: StateMaps,
}


impl<S: StateReader> CachedState<S> {
    pub fn new(state: S) -> Self {
        Self {
            state,
            cache: RefCell::new(StateCache::default()),
        }
    }

    pub fn update_cache(
        &mut self,
        write_updates: &StateMaps,
    ) {
        let mut cache = self.cache.borrow_mut();
        cache.writes.extend(write_updates);
    }
}

impl<S: StateReader> StateReader for CachedState<S> {
    fn get_storage_at(&self, contract_address: Address, key: U256) -> StateResult<U256> {
       self.state.get_storage_at(contract_address,key)
    }

    fn get_nonce_at(&self, contract_address: Address) -> StateResult<U256> {
        self.state.get_nonce_at(contract_address)
    }
}

impl<S: StateReader> UpdatableState for CachedState<S> {
    fn apply_writes(
        &mut self,
        writes: &StateMaps,
    ) {
        self.update_cache(writes);
    }
}