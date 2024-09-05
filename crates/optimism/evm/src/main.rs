// src/bin/my_test.rs

use std::collections::HashMap;
use std::str::FromStr;
use std::time::Instant;
use reth_chainspec::{ChainSpec, ChainSpecBuilder};
use reth_primitives::{U256, BlockWithSenders, Header};
use std::sync::Arc;
use revm::L1_BLOCK_CONTRACT;
use revm_primitives::address;
use revm_primitives::db::StateRef;
use reth_evm::execute::{BatchExecutor, BlockExecutorProvider, Executor};
use reth_evm_optimism::executor_provider;
use reth_execution_types::BlockExecutionInput;
use reth_revm::test_utils::StateProviderTest;
use reth_primitives::{
    b256, Account, Address, Block, Signature, StorageKey, StorageValue, Transaction,
    TransactionSigned, TxEip1559, BASE_MAINNET,
};
use reth_revm::database::{EvmStateProvider, StateProviderDatabase};
use reth_primitives::keccak256;
use sha3::{Digest, Keccak256};

fn create_op_state_provider() -> StateProviderTest {
    let mut db = StateProviderTest::default();

    let l1_block_contract_account =
        Account { balance: U256::ZERO, bytecode_hash: None, nonce: 1 };

    let mut l1_block_storage = HashMap::new();
    // base fee
    l1_block_storage.insert(StorageKey::with_last_byte(1), StorageValue::from(1000000000));
    // l1 fee overhead
    l1_block_storage.insert(StorageKey::with_last_byte(5), StorageValue::from(188));
    // l1 fee scalar
    l1_block_storage.insert(StorageKey::with_last_byte(6), StorageValue::from(684000));
    // l1 free scalars post ecotone
    l1_block_storage.insert(
        StorageKey::with_last_byte(3),
        StorageValue::from_str(
            "0x0000000000000000000000000000000000001db0000d27300000000000000005",
        )
            .unwrap(),
    );

    db.insert_account(L1_BLOCK_CONTRACT, l1_block_contract_account, None, l1_block_storage);

    db
}

fn main() {

    println!("Running test...");

    let header = Header {
        timestamp: 2,
        number: 1,
        gas_limit: 50_000_000,
        gas_used: 42_000,
        ..Default::default()
    };

    let mut db = create_op_state_provider();
   let addr = address!("d8da6bf26964af9d7eed9e03e53415d37aa96045");

    /// 1 eth
    let account = Account { balance: U256::from(1_000_000_000_000_000u64), ..Account::default() };

    db.insert_account(addr, account, None, HashMap::new());

    let chain_spec =
        Arc::new(ChainSpecBuilder::from(&*BASE_MAINNET).canyon_activated().build());

    let mut transactions = Vec::with_capacity(1500);
    let mut senders = Vec::with_capacity(1500);
    for i in 0..1 {
        let tx = TransactionSigned::from_transaction_and_signature(
            Transaction::Eip1559(TxEip1559 {
                chain_id: chain_spec.chain.id(),
                nonce: i as u64,
                gas_limit: 21_000,
                to: addr.into(),
                ..Default::default()
            }),
            Signature::default(),
        );
        transactions.push(tx);
        senders.push(addr);
    }

    for i in 0..1 {
        let tx_deposit = TransactionSigned::from_transaction_and_signature(
            Transaction::Deposit(reth_primitives::TxDeposit {
                from: addr,
                to: addr.into(),
                gas_limit: 21_000,
                value: U256::from(331100),    // test_db not implemented
                ..Default::default()
            }),
            Signature::optimism_deposit_tx_signature(),
        );
        transactions.push(tx_deposit);
        senders.push(addr);
    }

    let provider = executor_provider(chain_spec);
    let mut executor = provider.executor(StateProviderDatabase::new(&db));

    executor.state_mut().load_cache_account(L1_BLOCK_CONTRACT).unwrap();

    let start = Instant::now();

    let block_with_senders = BlockWithSenders {
        block: Block {
            header,
            body: transactions,
            ommers: vec![],
            withdrawals: None,
            requests: None,
        },
        senders,
    };
    let output = executor.execute(BlockExecutionInput {
        block: &block_with_senders,
        total_difficulty: U256::ZERO,
    }).unwrap();

    let tx_receipt = output.receipts;
    println!("tx receipt is : {:?}, root is {:?}", tx_receipt,block_with_senders.block.header.receipts_root);
    //let after_db = db.basic_account(addr).unwrap().unwrap();
    let state_after = output.state.account(&addr).unwrap();
    println!("after value is :{:?}",state_after);
    let duration = start.elapsed();
    println!("Single transaction execution time: {:?}", duration);
}
