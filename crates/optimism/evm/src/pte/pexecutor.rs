use std::fmt::Display;
use revm::db::CacheDB;
use revm::Evm;
use revm_primitives::{AccountInfo, Address, Bytecode, EVMError, ExecutionResult, KECCAK_EMPTY, ResultAndState, TxEnv, U256};
use revm_primitives::db::{Database, DatabaseCommit, DatabaseRef};
use reth_evm::execute::ProviderError;
use reth_execution_errors::BlockExecutionError;
use reth_revm::State;
use reth_primitives::{B256, Receipt};

pub struct Pexecutor {}

impl Pexecutor {
    /// all tx input
    pub fn execute() {}

    pub fn sequential_execute<DB, Ext>(
        storage: &State<DB>,
        mut evm: Evm<'_, Ext, &mut State<DB>>,
        txs: Vec<TxEnv>,
    ) -> Result<Vec<ExecutionResult>, EVMError<DB>>
    where
        DB: Database<Error: Into<ProviderError> + std::fmt::Display>,
    {
        let mut results = Vec::with_capacity(txs.len());
        let mut cumulative_gas_used: u64 = 0;
        for tx in txs {
            *evm.tx_mut() = tx;
            match evm.transact() {
                Ok(result_and_state) => {
                    let ResultAndState { result, state } = result_and_state;
                    evm.db_mut().commit(state);

                    // append gas used
                    cumulative_gas_used += result.gas_used();

                    results.push(result);
                }
                Err(err) => {}
            }
        }
        Ok(results)
    }

    /// multi worker run
    pub fn parallel_execute<DB: Send + Sync, Ext>(
        storage: &State<DB>,
        mut evm: Evm<'_, Ext, &mut State<DB>>,
        txs: Vec<TxEnv>,
    ) -> Result<Vec<ExecutionResult>, EVMError<DB>>
    where
        DB: Database<Error: Into<ProviderError> + std::fmt::Display>,
    {
     todo!()
    }

}


#[derive(Debug, Clone, PartialEq)]
pub struct AccountBasic {
    /// The balance of the account.
    pub balance: U256,
    /// The nonce of the account.
    pub nonce: u64,
}