mod pexecutor;
mod worker;
mod schedule;


/// index of tx
type TxIdx = usize;

/// incarnation of tx
type TxIncarnation = usize;
#[derive(Clone, Debug, PartialEq)]
pub struct TxVersion {
    tx_idx: TxIdx,
    tx_incarnation: TxIncarnation,
}

#[derive(Debug, PartialEq)]
pub enum Task {
    ExecutionTask(TxVersion),
    ValidationTask(TxVersion),
}


// - ReadyToExecute(i) --try_incarnate--> Executing(i)
// Non-blocked execution:
//   - Executing(i) --finish_execution--> Executed(i)
//   - Executed(i) --finish_validation--> Validated(i)
//   - Executed/Validated(i) --try_validation_abort--> Aborting(i)
//   - Aborted(i) --finish_validation(w.aborted=true)--> ReadyToExecute(i+1)
// Blocked execution:
//   - Executing(i) --add_dependency--> Aborting(i)
//   - Aborting(i) --resume--> ReadyToExecute(i+1)
#[derive(PartialEq, Debug)]
enum IncarnationStatus {
    ReadyToExecute,
    Executing,
    Executed,
    Validated,
    Aborting,
}

#[derive(PartialEq, Debug)]
struct TxStatus {
    incarnation: TxIncarnation,
    status: IncarnationStatus,
}