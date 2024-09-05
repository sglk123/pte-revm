use std::sync::Arc;
use revm_primitives::db::Database;
use revm_primitives::ExecutionResult;
use reth_evm::execute::ProviderError;
use reth_execution_errors::BlockExecutionError;
use reth_primitives::{BlockWithSenders, Receipt};
use reth_revm::{
    Evm, State,
};
use crate::api::StateReader;
use crate::cached_state::CachedState;
use crate::executor::{TransactionExecutorError, WorkerExecutor};
use crate::versioned_state::{ThreadSafeVersionedState, VersionedState};

pub struct ParallelExecutor<S: StateReader> {
    config: ParallelExecutionConfig,
    state: Option<CachedState<S>>,
}

pub struct ParallelExecutionConfig {
    pub enabled: bool,
    pub n_workers: usize,
}

impl<S: StateReader> ParallelExecutor<S> {
    pub fn new(
        state: CachedState<S>,
        config: ParallelExecutionConfig,
    ) -> Self {
        Self {
            config,
            state: Option::from(state),
        }
    }
    pub fn parallel_execute(
        &mut self,
        block: &BlockWithSenders) //todo add para input
        -> Vec<ExecutionResult>
    where
        S: StateReader + Send + Sync + 'static,
    {
        let state = self.state.take().expect("failed to take vs");
        let executor = Arc::new(WorkerExecutor::new(ThreadSafeVersionedState::new(VersionedState::new(state)), &[]));
        // todo add txs env


        // worker run
        std::thread::scope(|a| {
            for _ in 0..self.config.n_workers {
                let worker_executor = Arc::clone(&executor);
                a.spawn(move || {
                    worker_executor.run();
                    // todo panic handle
                });
            }
        });


        let n_committed_txs = executor.scheduler.get_n_committed_txs();
        let mut tx_execution_results = Vec::new();
        for execution_output in executor.execution_outputs.iter() {
            if tx_execution_results.len() >= n_committed_txs {
                break;
            }
            let locked_execution_output = execution_output
                .lock()
                .expect("Failed to lock execution output.")
                .take()
                .expect("Output must be ready.");

            tx_execution_results
                .push(locked_execution_output.result);  // todo tx error handling
        }

        let block_state_after_commit = Arc::try_unwrap(executor)
            .unwrap_or_else(|_| {
                std::panic!(
                    "To consume the block state, you must have only one strong reference to the \
                     worker executor factory. Consider dropping objects that hold a reference to \
                     it."
                )
            })
            .commit_and_recover_block_state(n_committed_txs);
        self.state.replace(block_state_after_commit);

        tx_execution_results
    }
}