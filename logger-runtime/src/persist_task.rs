//! Dedicated persistence task.
//!
//! Owns the `SqliteOpSink` and drains an unbounded mpsc channel of batched
//! log ops. The event loop sends ops without awaiting, so SQLite latency
//! never blocks UI responsiveness. On write failure, a `PersistError`
//! `AppEvent` is pushed back to the main loop so the operator can be warned
//! that the in-memory log has drifted from disk.
//!
//! The channel is **unbounded** on purpose: losing QSO records is
//! unacceptable. A runaway backlog grows memory instead of dropping data.
//! A backlog-size warning threshold is enforced at 128 items — if the
//! persist task ever falls that far behind, something is catastrophically
//! wrong with the disk (full FS, permissions issue, etc.) and the operator
//! needs to know.

use logger_core::AppEvent;
use qsolog::op::StoredOp;
use qsolog::persist::OpSink;
use qsolog::persist::sqlite::SqliteOpSink;
use tokio::sync::mpsc;
use tracing::{error, warn};

const BACKLOG_WARN_THRESHOLD: usize = 128;

/// Spawn the persist task, returning the sender used by `LogAdapter` to
/// enqueue op batches.
///
/// The task runs until `tx` is dropped (i.e., the adapter is dropped),
/// at which point it drains any remaining ops, flushes, and exits.
pub fn spawn_persist_task(
    sink: SqliteOpSink,
    err_tx: mpsc::Sender<AppEvent>,
) -> mpsc::UnboundedSender<Vec<StoredOp>> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(persist_task(sink, rx, err_tx));
    tx
}

async fn persist_task(
    mut sink: SqliteOpSink,
    mut rx: mpsc::UnboundedReceiver<Vec<StoredOp>>,
    err_tx: mpsc::Sender<AppEvent>,
) {
    while let Some(ops) = rx.recv().await {
        // Warn once if we're building a backlog — subsequent messages may
        // or may not fire depending on drain rate, but the threshold check
        // is per-wake, not stateful. That's fine for a diagnostic.
        let backlog = rx.len();
        if backlog >= BACKLOG_WARN_THRESHOLD {
            warn!(
                "persist task backlog = {backlog} batches; disk I/O is \
                 falling behind the event loop"
            );
        }

        if let Err(e) = sink.append_ops(&ops) {
            error!("persist append_ops failed: {e:?}");
            // Fire-and-forget error event to the main loop. If err_tx is
            // also full/closed, there's nothing more we can do — don't
            // panic the task.
            let _ = err_tx
                .send(AppEvent::PersistError {
                    message: format!("{e:?}"),
                })
                .await;
        }
    }
    // Channel closed — adapter dropped. Task exits naturally.
}
