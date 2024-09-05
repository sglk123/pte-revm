use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::Duration;
use revm::{Evm, EvmBuilder};
use revm_primitives::db::Database;
use revm_primitives::{EvmState, ExecutionResult, ResultAndState, TxEnv};
use thiserror::Error;
use crate::api::{StateError, StateReader, UpdatableState};
use crate::scheduler::{Scheduler, Task};
use crate::{OptimismEvmConfig, TxIndex};
use crate::versioned_state::{StateMaps, ThreadSafeVersionedState, VersionedStateProxy};


const EXECUTION_OUTPUTS_UNWRAP_ERROR: &str = "Execution task outputs should not be None.";


#[derive(Debug, Error)]
pub enum TransactionExecutorError {
    #[error("Transaction cannot be added to the current block, block capacity reached.")]
    BlockFull,
}

pub struct WorkerExecutor<'a, S: StateReader> {
    pub evm_config: OptimismEvmConfig,  // todo check, build evm
    pub scheduler: Scheduler,
    pub state: ThreadSafeVersionedState<S>,
    pub txs: &'a [TxEnv],
    pub execution_outputs: Box<[Mutex<Option<ExecutionTaskOutput>>]>,
}

#[derive(Debug)]
pub struct ExecutionTaskOutput {
    pub reads: StateMaps,
    pub writes: StateMaps,
    pub result: ExecutionResult,
}

impl<'a, S: StateReader> WorkerExecutor<'a, S> {
    pub fn new(
        state: ThreadSafeVersionedState<S>,
        txs: &'a [TxEnv],
    ) -> Self {
        let scheduler = Scheduler::new(txs.len());
        let execution_outputs =
            std::iter::repeat_with(|| Mutex::new(None)).take(txs.len()).collect();

        WorkerExecutor {
            evm_config: Default::default(), //todo evm build?
            scheduler,
            state,
            txs,
            execution_outputs,
        }
    }
    pub fn run(&self) {
        let mut task = Task::AskForTask;
        loop {
             self.commit_while_possible();
            task = match task {
                Task::ExecutionTask(tx_index) => {
                    self.execute(tx_index);
                    Task::AskForTask
                }
                Task::ValidationTask(tx_index) => self.validate(tx_index),
                Task::NoTaskAvailable => {
                    // There's no available task at the moment; sleep for a bit to save CPU power.
                    // (since busy-looping might damage performance when using hyper-threads).
                    thread::sleep(Duration::from_micros(1));
                    Task::AskForTask
                }
                Task::AskForTask => self.scheduler.next_task(),
                Task::Done => break,
            };
        }
    }

    fn commit_while_possible(&self) {
        if let Some(mut transaction_committer) = self.scheduler.try_enter_commit_phase() {
            while let Some(tx_index) = transaction_committer.try_commit() {
                let commit_succeeded = self.commit_tx(tx_index);
                if !commit_succeeded {
                    transaction_committer.halt_scheduler();
                }
            }
        }
    }

    fn validate(&self, tx_index: TxIndex) -> Task {
        println!("sglk validate {}",tx_index);
        let tx_versioned_state = self.state.pin_version(tx_index);
        let execution_output = lock_mutex_in_array(&self.execution_outputs, tx_index);
        let execution_output = execution_output.as_ref().expect(EXECUTION_OUTPUTS_UNWRAP_ERROR);
        let reads = &execution_output.reads;
        let reads_valid = tx_versioned_state.validate_reads(reads);

        let aborted = !reads_valid && self.scheduler.try_validation_abort(tx_index);
        if aborted {
            tx_versioned_state
                .delete_writes(&execution_output.writes);
            self.scheduler.finish_abort(tx_index)
        } else {
            Task::AskForTask
        }
    }

    fn commit_tx(&self, tx_index: TxIndex) -> bool {
        /// validate read set
        let execution_output = lock_mutex_in_array(&self.execution_outputs, tx_index);
        let execution_output_ref = execution_output.as_ref().expect(EXECUTION_OUTPUTS_UNWRAP_ERROR);
        let reads = &execution_output_ref.reads;

        let mut tx_versioned_state = self.state.pin_version(tx_index);
        let reads_valid = tx_versioned_state.validate_reads(reads);


        // First, re-validate the transaction.
        if !reads_valid {
            // Revalidate failed: re-execute the transaction.
            tx_versioned_state.delete_writes(
                &execution_output_ref.writes,
            );
            // Release the execution output lock as it is acquired in execution (avoid dead-lock).
            drop(execution_output);

            self.execute_tx(tx_index);
            self.scheduler.finish_execution_during_commit(tx_index);

            let execution_output = lock_mutex_in_array(&self.execution_outputs, tx_index);
            let read_set = &execution_output.as_ref().expect(EXECUTION_OUTPUTS_UNWRAP_ERROR).reads;
            // Another validation after the re-execution for sanity check.
            assert!(tx_versioned_state.validate_reads(read_set));
        } else {
            // Release the execution output lock, since it is has been released in the other flow.
            drop(execution_output);
        }


        /// todo bouncer result?
        true
    }
    fn execute(&self, tx_index: TxIndex) {
        self.execute_tx(tx_index);
        self.scheduler.finish_execution(tx_index)
    }

    fn execute_tx(&self, tx_index: TxIndex) {
        let mut evm = build_evm(tx_index, &self.state);
        /// evm insert txenv
        let tx = &self.txs[tx_index];
        *evm.tx_mut() = tx.clone();

        match evm.transact() {
            Ok(result_and_state) => {
                let ResultAndState { result, state } = result_and_state;
                let (readset, writeset) = transfer_to_statemap(&state);
                self.state.pin_version(tx_index).apply_writes(&writeset);
                let mut execution_output = lock_mutex_in_array(&self.execution_outputs, tx_index);
                *execution_output = Some(ExecutionTaskOutput {
                    reads: readset,
                    writes: writeset,
                    result,
                });
            }
            Err(err) => {}
        }
    }
}

/// todo nonce handle
fn transfer_to_statemap(evm_state: &EvmState) -> (StateMaps, StateMaps) {
    let mut read_set = StateMaps {
        nonces: HashMap::new(),
        storage: HashMap::new(),
    };
    let mut write_set = StateMaps {
        nonces: HashMap::new(),
        storage: HashMap::new(),
    };
    for (address, account) in evm_state {
        //  read_set.nonces.insert(*address, account.info.nonce);

        for (slot_key, storage_slot) in &account.storage {
            if storage_slot.original_value == storage_slot.present_value {
                read_set.storage.insert((*address, *slot_key), storage_slot.original_value);
            } else {
                write_set.storage.insert((*address, *slot_key), storage_slot.present_value);
            }
        }
    }

    (read_set, write_set)
}

pub fn lock_mutex_in_array<T: Debug>(array: &[Mutex<T>], tx_index: TxIndex) -> MutexGuard<'_, T> {
    array[tx_index].lock().unwrap_or_else(|error| {
        panic!("Cell of transaction index {} is poisoned. Data: {:?}.", tx_index, *error.get_ref())
    })
}

fn build_evm<'b, S: StateReader>(tx_index: TxIndex, db: &ThreadSafeVersionedState<S>) -> Evm<'b, (), VersionedStateProxy<S>> {
    //todo complete
    let tx_versioned_state = db.pin_version(tx_index);
    EvmBuilder::default().with_db::<VersionedStateProxy<S>>(tx_versioned_state).optimism().build()
}

impl<'a, U: UpdatableState> WorkerExecutor<'a, U> {
    pub fn commit_and_recover_block_state(
        self,
        n_committed_txs: usize,
    ) -> U {
        self.state
            .into_inner_state()
            .commit_and_recover_block_state(n_committed_txs)
    }
}