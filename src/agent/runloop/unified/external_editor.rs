use std::future::Future;
use std::time::Duration;

use anyhow::Result;
use vtcode_ui::tui::app::InlineHandle;

/// Gives the external application exclusive access to the terminal while it runs.
///
/// The guard restores event processing from `Drop`, so cancellation of the
/// surrounding task cannot leave the fullscreen TUI suspended.
pub(crate) async fn run_with_event_loop_suspended<T, F>(
    handle: &InlineHandle,
    suspend_tui: bool,
    launch: F,
) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    let guard = TuiSuspensionGuard::new(handle, suspend_tui);
    if guard.is_active() {
        tokio::time::sleep(EVENT_STREAM_STOP_DELAY).await;
        handle.clear_input_queue();
    }

    let result = launch();
    drop(guard);
    result
}

pub(crate) async fn run_with_event_loop_suspended_async<T, F, Fut>(
    handle: &InlineHandle,
    suspend_tui: bool,
    launch: F,
) -> Result<T>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let guard = TuiSuspensionGuard::new(handle, suspend_tui);
    if guard.is_active() {
        tokio::time::sleep(EVENT_STREAM_STOP_DELAY).await;
        handle.clear_input_queue();
    }

    let result = launch().await;
    drop(guard);
    result
}

const EVENT_STREAM_STOP_DELAY: Duration = Duration::from_millis(150);

struct TuiSuspensionGuard<'a> {
    handle: &'a InlineHandle,
    active: bool,
}

impl<'a> TuiSuspensionGuard<'a> {
    fn new(handle: &'a InlineHandle, active: bool) -> Self {
        if active {
            // Fully stop the background EventStream task so terminal editors
            // have exclusive access to stdin.
            handle.stop_event_stream();
        }
        Self { handle, active }
    }

    fn is_active(&self) -> bool {
        self.active
    }
}

impl Drop for TuiSuspensionGuard<'_> {
    fn drop(&mut self) {
        if self.active {
            self.handle.clear_input_queue();
            self.handle.resume_event_loop();
            self.handle.start_event_stream();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::{mpsc, oneshot};
    use vtcode_ui::tui::app::{InlineCommand, InlineHandle};

    #[tokio::test]
    async fn cancellation_restores_suspended_event_loop() {
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(command_tx);
        let (started_tx, started_rx) = oneshot::channel();
        let task_handle = handle.clone();
        let task = tokio::spawn(async move {
            run_with_event_loop_suspended_async(&task_handle, true, || async move {
                let _ = started_tx.send(());
                std::future::pending::<Result<()>>().await
            })
            .await
        });

        started_rx.await.expect("launch closure should start");
        task.abort();
        task.await.expect_err("suspended operation should be cancelled");

        assert!(matches!(command_rx.try_recv(), Ok(InlineCommand::StopEventStream)));
        assert!(matches!(command_rx.try_recv(), Ok(InlineCommand::ClearInputQueue)));
        assert!(matches!(command_rx.try_recv(), Ok(InlineCommand::ClearInputQueue)));
        assert!(matches!(command_rx.try_recv(), Ok(InlineCommand::ResumeEventLoop)));
        assert!(matches!(command_rx.try_recv(), Ok(InlineCommand::StartEventStream)));
    }
}
