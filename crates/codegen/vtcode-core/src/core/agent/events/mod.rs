//! Event recording utilities for the agent runner.

mod lifecycle;
pub use lifecycle::{
    SharedLifecycleEmitter, ToolOutputPayload, error_item_completed_event, tool_invocation_completed_event,
    tool_output_completed_event, tool_output_item_id, tool_output_payload_from_value, tool_output_started_event,
    tool_output_updated_event, tool_started_event,
};

use crate::core::threads::{SubmissionId, ThreadRuntimeHandle};
use crate::exec::events::{
    CommandExecutionItem, CommandExecutionStatus, CompactionMode, CompactionTrigger, ErrorItem, FileChangeItem,
    FileUpdateChange, HarnessEventItem, HarnessEventKind, ItemCompletedEvent, ItemStartedEvent, PatchApplyStatus,
    PatchChangeKind, ThreadCompactBoundaryEvent, ThreadCompletedEvent, ThreadCompletionSubtype, ThreadEvent,
    ThreadItem, ThreadItemDetails, ThreadStartedEvent, ToolOutcome, TurnCompletedEvent, TurnFailedEvent,
    TurnStartedEvent, Usage, tool_outcome_from_status,
};
use anyhow::{Context, Result, anyhow};
use parking_lot::Mutex;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::task::spawn_blocking;
use uuid::Uuid;

use vtcode_memory::event_log::DEFAULT_MAX_EVENTS;

const SESSION_STORE_DRAIN_CAPACITY: usize = 8192;

/// Callback type alias for streaming structured events.
pub type EventSink = Arc<Mutex<Box<dyn FnMut(&ThreadEvent) + Send>>>;

#[derive(Debug, Default)]
struct SessionStoreSinkHealth {
    accepted_events: AtomicU64,
    persisted_events: AtomicU64,
    append_failures: AtomicU64,
    channel_failures: AtomicU64,
    failed: AtomicBool,
    closing: AtomicBool,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SessionStoreSinkHealthSnapshot {
    accepted_events: u64,
    persisted_events: u64,
    append_failures: u64,
    channel_failures: u64,
    failed: bool,
}

impl SessionStoreSinkHealth {
    #[cfg(test)]
    fn snapshot(&self) -> SessionStoreSinkHealthSnapshot {
        SessionStoreSinkHealthSnapshot {
            accepted_events: self.accepted_events.load(Ordering::Relaxed),
            persisted_events: self.persisted_events.load(Ordering::Relaxed),
            append_failures: self.append_failures.load(Ordering::Relaxed),
            channel_failures: self.channel_failures.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
        }
    }
}

/// Owns the session persistence drain and exposes its final health result.
///
/// The event callback remains synchronous for compatibility, while the runner
/// retains this handle so a completed task cannot report success before its
/// authoritative event queue has drained.
pub(crate) struct SessionStoreSinkHandle {
    health: Arc<SessionStoreSinkHealth>,
    drain: JoinHandle<Result<()>>,
}

impl SessionStoreSinkHandle {
    pub(crate) async fn close(self) -> Result<()> {
        self.health.closing.store(true, Ordering::Release);
        self.drain.await.context("session event drain task failed")??;
        if self.health.failed.load(Ordering::Acquire) {
            return Err(anyhow!("session event persistence failed"));
        }
        Ok(())
    }
}

#[doc(hidden)]
pub fn event_sink<F>(callback: F) -> EventSink
where
    F: FnMut(&ThreadEvent) + Send + 'static,
{
    Arc::new(Mutex::new(Box::new(callback)))
}

/// Build an event sink that persists every recorded event to the unified
/// per-session store ([`vtcode_memory`]), making it the canonical
/// source of truth for session state/history. Returns `None` (non-fatally) if
/// the store cannot be opened.
///
/// The sink hands events to a bounded background drain. The handoff waits
/// when the queue is full because session events are authoritative and must
/// not be silently dropped; disk I/O and manifest writes remain off the
/// Tokio runtime worker through `spawn_blocking`.
pub fn session_store_sink(workspace: &Path, session_id: &str) -> Option<EventSink> {
    session_store_sink_with_capacity_handle(workspace, session_id, SESSION_STORE_DRAIN_CAPACITY).map(|(sink, _)| sink)
}

pub(crate) fn session_store_sink_with_handle(
    workspace: &Path,
    session_id: &str,
) -> Option<(EventSink, SessionStoreSinkHandle)> {
    session_store_sink_with_capacity_handle(workspace, session_id, SESSION_STORE_DRAIN_CAPACITY)
}

fn session_store_sink_with_capacity_handle(
    workspace: &Path,
    session_id: &str,
    capacity: usize,
) -> Option<(EventSink, SessionStoreSinkHandle)> {
    let log = match vtcode_memory::open(workspace, session_id, DEFAULT_MAX_EVENTS) {
        Ok(log) => log,
        Err(err) => {
            tracing::warn!("session store unavailable for {session_id}: {err}");
            return None;
        }
    };

    let (tx, rx) = mpsc::sync_channel::<ThreadEvent>(capacity.max(1));
    let health = Arc::new(SessionStoreSinkHealth::default());
    let drain_health = Arc::clone(&health);
    let drain_session_id = session_id.to_string();

    // Start the blocking receiver directly so a full synchronous handoff
    // cannot wait for a Tokio worker that is currently doing the send.
    let drain_handle = spawn_blocking(move || drain_session_events(rx, log, drain_session_id, drain_health));
    let drain = tokio::spawn(async move {
        drain_handle.await.context("blocking session event drain failed")?;
        Ok::<(), anyhow::Error>(())
    });

    let sink_health = Arc::clone(&health);
    Some((
        event_sink(move |event: &ThreadEvent| {
            if sink_health.failed.load(Ordering::Acquire) || sink_health.closing.load(Ordering::Acquire) {
                sink_health.channel_failures.fetch_add(1, Ordering::Relaxed);
                tracing::error!("session event persistence is unavailable; event was not accepted");
                return;
            }
            match tx.send(event.clone()) {
                Ok(()) => {
                    sink_health.accepted_events.fetch_add(1, Ordering::Relaxed);
                }
                Err(err) => {
                    sink_health.failed.store(true, Ordering::Release);
                    sink_health.channel_failures.fetch_add(1, Ordering::Relaxed);
                    tracing::error!(
                        error = %err,
                        "session event persistence channel closed before event was accepted"
                    );
                }
            }
        }),
        SessionStoreSinkHandle { health, drain },
    ))
}

fn drain_session_events(
    rx: Receiver<ThreadEvent>,
    log: vtcode_memory::SessionEventLog,
    session_id: String,
    health: Arc<SessionStoreSinkHealth>,
) {
    loop {
        let event = match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) if health.closing.load(Ordering::Acquire) => break,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match log.append(&event) {
            Ok(()) => {
                health.persisted_events.fetch_add(1, Ordering::Relaxed);
            }
            Err(err) => {
                health.append_failures.fetch_add(1, Ordering::Relaxed);
                health.failed.store(true, Ordering::Release);
                tracing::error!(
                    session_id = %session_id,
                    error = %err,
                    "failed to persist session event; stopping authoritative drain"
                );
                break;
            }
        }
    }

    if let Err(err) = log.flush() {
        health.append_failures.fetch_add(1, Ordering::Relaxed);
        health.failed.store(true, Ordering::Release);
        tracing::error!(
            session_id = %session_id,
            error = %err,
            "failed to flush session event log during drain shutdown"
        );
    }
}

/// Combine two optional event sinks into one that fans out to both.
pub fn combine_event_sinks(a: Option<EventSink>, b: Option<EventSink>) -> Option<EventSink> {
    match (a, b) {
        (None, None) => None,
        (Some(s), None) | (None, Some(s)) => Some(s),
        (Some(a), Some(b)) => Some(event_sink(move |e: &ThreadEvent| {
            a.lock()(e);
            b.lock()(e);
        })),
    }
}

#[derive(Debug, Clone)]
pub struct ActiveCommandHandle {
    id: String,
    command: String,
}

#[derive(Debug, Clone)]
pub struct ActiveToolHandle {
    id: String,
    tool_name: String,
    arguments: Option<Value>,
    tool_call_id: Option<String>,
}

impl ActiveToolHandle {
    #[must_use]
    pub fn item_id(&self) -> &str {
        &self.id
    }
}

/// Helper responsible for recording execution events and relaying them to optional sinks.
#[derive(Default)]
pub struct ExecEventRecorder {
    thread_id: String,
    events: Vec<ThreadEvent>,
    event_sink: Option<EventSink>,
    thread_handle: Option<ThreadRuntimeHandle>,
    active_submission_id: Option<SubmissionId>,
    active_turn_id: Option<String>,
    lifecycle: SharedLifecycleEmitter,
}

impl ExecEventRecorder {
    pub fn new(
        thread_id: impl Into<String>,
        event_sink: Option<EventSink>,
        thread_handle: Option<ThreadRuntimeHandle>,
    ) -> Self {
        let thread_id = thread_id.into();
        let mut recorder = Self {
            thread_id: thread_id.clone(),
            events: Vec::new(),
            event_sink,
            thread_handle,
            active_submission_id: None,
            active_turn_id: None,
            lifecycle: SharedLifecycleEmitter::default(),
        };
        recorder.record_with_context(None, None, ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id }));
        recorder
    }

    fn record(&mut self, event: ThreadEvent) {
        self.record_with_context(self.active_submission_id.clone(), self.active_turn_id.clone(), event);
    }

    fn record_with_context(
        &mut self,
        submission_id: Option<SubmissionId>,
        turn_id: Option<String>,
        event: ThreadEvent,
    ) {
        if let Some(sink) = &self.event_sink {
            let mut callback = sink.lock();
            callback(&event);
        }
        if let Some(handle) = &self.thread_handle {
            handle.record_event(submission_id, turn_id, event.clone());
        }
        self.events.push(event);
    }

    pub fn record_thread_event(&mut self, event: ThreadEvent) {
        self.record(event);
    }

    pub fn record_thread_events<I>(&mut self, events: I)
    where
        I: IntoIterator<Item = ThreadEvent>,
    {
        for event in events {
            self.record(event);
        }
    }

    fn record_pending_lifecycle_events(&mut self) {
        for event in self.lifecycle.drain_events() {
            self.record(event);
        }
    }

    fn next_item_id(&mut self) -> String {
        self.lifecycle.next_item_id()
    }

    pub fn turn_started(&mut self) {
        if let Some(handle) = &self.thread_handle {
            match handle.begin_turn() {
                Ok(submission_id) => self.active_submission_id = Some(submission_id),
                Err(_) => self.active_submission_id = None,
            }
            self.active_turn_id = Some(format!("turn-{}", Uuid::new_v4()));
        }
        self.record(ThreadEvent::TurnStarted(TurnStartedEvent::default()));
    }

    pub fn turn_completed(&mut self) {
        self.record(ThreadEvent::TurnCompleted(TurnCompletedEvent { usage: Usage::default() }));
        self.finish_turn();
    }

    pub fn turn_failed(&mut self, message: &str) {
        self.record(ThreadEvent::TurnFailed(TurnFailedEvent { message: message.to_string(), usage: None }));
        self.finish_turn();
    }

    pub fn thread_completed(
        &mut self,
        session_id: &str,
        subtype: ThreadCompletionSubtype,
        outcome_code: &str,
        result: Option<&str>,
        stop_reason: Option<&str>,
        usage: Usage,
        total_cost_usd: Option<serde_json::Number>,
        num_turns: usize,
    ) {
        self.record(ThreadEvent::ThreadCompleted(ThreadCompletedEvent {
            thread_id: self.thread_id.clone(),
            session_id: session_id.to_string(),
            subtype,
            outcome_code: outcome_code.to_string(),
            result: result.map(str::to_string),
            stop_reason: stop_reason.map(str::to_string),
            usage,
            total_cost_usd,
            num_turns,
        }));
    }

    pub fn compact_boundary(
        &mut self,
        trigger: CompactionTrigger,
        mode: CompactionMode,
        original_message_count: usize,
        compacted_message_count: usize,
        history_artifact_path: Option<&str>,
    ) {
        self.record(ThreadEvent::ThreadCompactBoundary(ThreadCompactBoundaryEvent {
            thread_id: self.thread_id.clone(),
            trigger,
            mode,
            original_message_count,
            compacted_message_count,
            history_artifact_path: history_artifact_path.map(str::to_string),
            previous_segment_id: None,
            new_segment_id: None,
            previous_prefix_hash: None,
            new_prefix_hash: None,
            previous_catalog_hash: None,
            new_catalog_hash: None,
        }));
    }

    fn finish_turn(&mut self) {
        if let Some(handle) = &self.thread_handle {
            handle.finish_turn();
        }
        self.active_submission_id = None;
        self.active_turn_id = None;
    }

    pub fn agent_message(&mut self, text: &str) {
        self.lifecycle.emit_completed_agent_message(text);
        self.record_pending_lifecycle_events();
    }

    pub fn agent_message_stream_update(&mut self, text: &str) -> bool {
        if text.trim().is_empty() || !self.lifecycle.replace_assistant_text(text) {
            return false;
        }
        let emitted = self.lifecycle.emit_assistant_snapshot(None);
        self.record_pending_lifecycle_events();
        emitted
    }

    pub fn agent_message_stream_complete(&mut self) {
        let _ = self.lifecycle.complete_assistant_stream();
        self.record_pending_lifecycle_events();
    }

    pub fn reasoning(&mut self, text: &str) {
        self.lifecycle.emit_completed_reasoning(text);
        self.record_pending_lifecycle_events();
    }

    pub fn set_reasoning_stage(&mut self, stage: &str) {
        if !self.lifecycle.set_reasoning_stage(Some(stage.to_string())) {
            return;
        }
        let _ = self.lifecycle.emit_reasoning_stage_update();
        self.record_pending_lifecycle_events();
    }

    pub fn reasoning_stream_update(&mut self, text: &str) -> bool {
        if text.trim().is_empty() || !self.lifecycle.replace_reasoning_text(text) {
            return false;
        }
        let emitted = self.lifecycle.emit_reasoning_snapshot(None);
        self.record_pending_lifecycle_events();
        emitted
    }

    pub fn reasoning_stream_complete(&mut self) {
        let _ = self.lifecycle.complete_reasoning_stream();
        self.record_pending_lifecycle_events();
    }

    pub fn tool_started(
        &mut self,
        tool_name: &str,
        arguments: Option<&Value>,
        tool_call_id: Option<&str>,
    ) -> ActiveToolHandle {
        let handle = ActiveToolHandle {
            id: self.next_item_id(),
            tool_name: tool_name.to_string(),
            arguments: arguments.cloned(),
            tool_call_id: tool_call_id.map(str::to_string),
        };
        self.record(tool_started_event(
            handle.id.clone(),
            &handle.tool_name,
            handle.arguments.as_ref(),
            handle.tool_call_id.as_deref(),
        ));
        handle
    }

    pub fn tool_finished(
        &mut self,
        handle: &ActiveToolHandle,
        status: crate::exec::events::ToolCallStatus,
        exit_code: Option<i32>,
        aggregated_output: &str,
        spool_path: Option<&str>,
    ) {
        let outcome = tool_outcome_from_status(&status);
        self.record(tool_invocation_completed_event(
            handle.id.clone(),
            &handle.tool_name,
            handle.arguments.as_ref(),
            handle.tool_call_id.as_deref(),
            status.clone(),
            outcome,
        ));
        self.record(tool_output_completed_event(
            handle.id.clone(),
            handle.tool_call_id.as_deref(),
            status,
            exit_code,
            spool_path,
            aggregated_output,
        ));
    }

    pub fn tool_output_started(&mut self, call_item_id: &str, tool_call_id: Option<&str>) {
        self.record(tool_output_started_event(call_item_id.to_string(), tool_call_id));
    }

    pub fn tool_output_updated(&mut self, call_item_id: &str, tool_call_id: Option<&str>, output: &str) {
        self.record(tool_output_updated_event(call_item_id.to_string(), tool_call_id, output));
    }

    pub fn tool_output_finished(
        &mut self,
        call_item_id: &str,
        tool_call_id: Option<&str>,
        status: crate::exec::events::ToolCallStatus,
        exit_code: Option<i32>,
        aggregated_output: &str,
        spool_path: Option<&str>,
    ) {
        self.record(tool_output_completed_event(
            call_item_id.to_string(),
            tool_call_id,
            status,
            exit_code,
            spool_path,
            aggregated_output,
        ));
    }

    pub fn tool_rejected(
        &mut self,
        tool_name: &str,
        arguments: Option<&Value>,
        tool_call_id: Option<&str>,
        detail: &str,
    ) {
        let handle = self.tool_started(tool_name, arguments, tool_call_id);
        let call_item_id = handle.id.clone();
        self.record(tool_invocation_completed_event(
            call_item_id.clone(),
            tool_name,
            arguments,
            tool_call_id,
            crate::exec::events::ToolCallStatus::Failed,
            ToolOutcome::HookDenied,
        ));
        self.record(tool_output_started_event(call_item_id.clone(), tool_call_id));
        self.record(tool_output_completed_event(
            call_item_id,
            tool_call_id,
            crate::exec::events::ToolCallStatus::Failed,
            None,
            None,
            detail,
        ));
        let error_item_id = self.next_item_id();
        self.record(error_item_completed_event(error_item_id, detail.to_string()));
    }

    pub fn permission_requested(&mut self, tool_name: &str) {
        self.record(ThreadEvent::PermissionRequested(crate::exec::events::PermissionRequestedEvent {
            tool_name: tool_name.to_string(),
        }));
    }

    pub fn permission_resolved(
        &mut self,
        tool_name: &str,
        decision: crate::exec::events::PermissionDecision,
        wait_ms: u64,
    ) {
        self.record(ThreadEvent::PermissionResolved(crate::exec::events::PermissionResolvedEvent {
            tool_name: tool_name.to_string(),
            decision,
            wait_ms,
        }));
    }

    pub fn command_started(&mut self, command: &str) -> ActiveCommandHandle {
        let id = self.next_item_id();
        let item = ThreadItem {
            id: id.clone(),
            details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                command: command.to_string(),
                arguments: None,
                aggregated_output: String::new(),
                exit_code: None,
                status: CommandExecutionStatus::InProgress,
            })),
        };
        self.record(ThreadEvent::ItemStarted(ItemStartedEvent { item }));
        ActiveCommandHandle { id, command: command.to_string() }
    }

    pub fn command_finished(
        &mut self,
        handle: &ActiveCommandHandle,
        status: CommandExecutionStatus,
        exit_code: Option<i32>,
        aggregated_output: &str,
    ) {
        let item = ThreadItem {
            id: handle.id.clone(),
            details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                command: handle.command.clone(),
                arguments: None,
                aggregated_output: aggregated_output.to_string(),
                exit_code,
                status,
            })),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    pub fn file_change_completed(&mut self, path: &str) {
        let change = FileUpdateChange {
            path: path.to_string(),
            kind: PatchChangeKind::Update,
        };
        let item = ThreadItem {
            id: self.next_item_id(),
            details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
                changes: vec![change],
                status: PatchApplyStatus::Completed,
            })),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    pub fn warning(&mut self, message: &str) {
        let item = ThreadItem {
            id: self.next_item_id(),
            details: ThreadItemDetails::Error(ErrorItem { message: message.to_string() }),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    pub fn harness_event(
        &mut self,
        event: HarnessEventKind,
        message: Option<String>,
        command: Option<String>,
        path: Option<String>,
        exit_code: Option<i32>,
        attempt: Option<u32>,
        error_category: Option<String>,
    ) {
        let item = ThreadItem {
            id: self.next_item_id(),
            details: ThreadItemDetails::Harness(HarnessEventItem {
                event,
                message,
                command,
                path,
                exit_code,
                attempt,
                error_category,
                duration_ms: None,
            }),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    /// Emit a tool latency harness event with recorded duration.
    pub fn record_tool_latency(&mut self, tool_name: &str, duration_ms: u64) {
        let item = ThreadItem {
            id: self.next_item_id(),
            details: ThreadItemDetails::Harness(HarnessEventItem {
                event: HarnessEventKind::ToolLatencyRecorded,
                message: Some(format!("{tool_name} completed in {duration_ms}ms")),
                command: None,
                path: None,
                exit_code: None,
                attempt: None,
                error_category: None,
                duration_ms: Some(duration_ms),
            }),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    /// Emit an `ErrorRecovered` harness event, recording that the agent
    /// successfully recovered from a transient error after retries.
    pub fn error_recovered(&mut self, tool_name: &str, attempt: u32, error_category: &str) {
        self.harness_event(
            HarnessEventKind::ErrorRecovered,
            Some(format!("{tool_name} recovered after {attempt} retries")),
            None,
            None,
            None,
            Some(attempt),
            Some(error_category.to_string()),
        );
    }

    /// Emit a `ToolRetryAttempted` harness event, recording that a transient
    /// tool failure triggered an automatic retry.
    pub fn tool_retry_attempted(&mut self, tool_name: &str, attempt: u32, error_category: &str, delay_ms: u64) {
        self.harness_event(
            HarnessEventKind::ToolRetryAttempted,
            Some(format!("{tool_name}: retry {attempt} after {delay_ms}ms")),
            None,
            None,
            None,
            Some(attempt),
            Some(error_category.to_string()),
        );
    }

    pub fn into_events(mut self) -> Vec<ThreadEvent> {
        self.lifecycle.complete_open_items();
        self.record_pending_lifecycle_events();
        self.events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::threads::{ThreadBootstrap, ThreadManager};
    use std::fs;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::time::timeout;

    fn make_recorder() -> ExecEventRecorder {
        ExecEventRecorder::new("thread", None, None)
    }

    #[tokio::test]
    async fn session_store_sink_preserves_order_when_queue_is_small() {
        let workspace = TempDir::new().expect("workspace");
        let (sink, handle) =
            session_store_sink_with_capacity_handle(workspace.path(), "session", 1).expect("session sink");
        let health = Arc::clone(&handle.health);
        let events = vec![
            ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "thread".to_string() }),
            ThreadEvent::TurnStarted(TurnStartedEvent::default()),
            ThreadEvent::TurnCompleted(TurnCompletedEvent { usage: Usage::default() }),
        ];

        {
            let mut callback = sink.lock();
            for event in &events {
                callback(event);
            }
        }
        drop(sink);

        timeout(Duration::from_secs(5), handle.close())
            .await
            .expect("session sink should drain before timeout")
            .expect("session sink should close successfully");

        assert_eq!(health.snapshot().accepted_events, events.len() as u64);
        let log = vtcode_memory::open(workspace.path(), "session", DEFAULT_MAX_EVENTS).expect("reopen session");
        assert_eq!(log.event_count(), events.len() as u64);
        let event_path = workspace.path().join(".vtcode/sessions/session/events.jsonl");
        let persisted = fs::read_to_string(event_path)
            .expect("read persisted events")
            .lines()
            .map(|line| serde_json::from_str::<vtcode_exec_events::VersionedThreadEvent>(line).expect("decode event"))
            .map(vtcode_exec_events::VersionedThreadEvent::into_event)
            .collect::<Vec<_>>();
        assert_eq!(persisted, events);
        assert_eq!(health.snapshot().append_failures, 0);
        assert_eq!(health.snapshot().channel_failures, 0);
        assert!(!health.snapshot().failed);
    }

    #[test]
    fn closed_session_store_channel_is_observable() {
        let (sender, receiver) = mpsc::sync_channel(1);
        drop(receiver);
        let health = SessionStoreSinkHealth::default();
        let event = ThreadEvent::TurnStarted(TurnStartedEvent::default());

        if sender.send(event).is_err() {
            health.channel_failures.fetch_add(1, Ordering::Relaxed);
        }

        assert_eq!(health.snapshot().channel_failures, 1);
    }

    #[test]
    fn streaming_events_flush_on_completion() {
        let mut recorder = make_recorder();
        recorder.turn_started();
        assert!(recorder.agent_message_stream_update("partial"));
        recorder.agent_message_stream_complete();
        let events = recorder.into_events();
        assert!(events.iter().any(|event| matches!(event, ThreadEvent::ItemCompleted(_))));
    }

    #[test]
    fn command_events_capture_status() {
        let mut recorder = make_recorder();
        let handle = recorder.command_started("git status");
        recorder.command_finished(&handle, CommandExecutionStatus::Completed, Some(0), "");
        let events = recorder.into_events();
        let command = events
            .into_iter()
            .filter_map(|event| match event {
                ThreadEvent::ItemCompleted(event) => Some(event.item),
                _ => None,
            })
            .find(|item| matches!(item.details, ThreadItemDetails::CommandExecution(_)))
            .expect("command event should be emitted");

        match command.details {
            ThreadItemDetails::CommandExecution(details) => {
                assert_eq!(details.command, "git status");
                assert_eq!(details.status, CommandExecutionStatus::Completed);
            }
            _ => panic!("unexpected event variant"),
        }
    }

    #[test]
    fn rejected_tool_call_emits_failed_tool_output_item() {
        let mut recorder = make_recorder();
        recorder.tool_rejected("read_file", None, Some("call_1"), "Tool permission denied");

        let events = recorder.into_events();
        let tool_outputs = events
            .iter()
            .filter_map(|event| match event {
                ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) => match &item.details {
                    ThreadItemDetails::ToolOutput(details) => Some(details),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(tool_outputs.len(), 1);
        assert_eq!(tool_outputs[0].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(tool_outputs[0].status, crate::exec::events::ToolCallStatus::Failed);
        assert_eq!(tool_outputs[0].output, "Tool permission denied");
    }

    #[test]
    fn thread_backed_recorder_reuses_submission_id_within_turn() {
        let handle = ThreadManager::new().start_thread_with_identifier("thread", ThreadBootstrap::new(None));
        let mut recorder = ExecEventRecorder::new("thread", None, Some(handle.clone()));

        recorder.turn_started();
        recorder.agent_message("hello");
        recorder.turn_completed();

        let records = handle.replay_recent();
        let submission_ids: std::collections::BTreeSet<String> = records
            .iter()
            .filter_map(|record| record.submission_id.as_ref().map(|id| id.as_str().to_string()))
            .collect();

        assert_eq!(submission_ids.len(), 1);
        assert!(
            records
                .iter()
                .any(|record| matches!(record.event, ThreadEvent::TurnStarted(_)) && record.submission_id.is_some())
        );
        assert!(
            records
                .iter()
                .any(|record| matches!(record.event, ThreadEvent::TurnCompleted(_)) && record.submission_id.is_some())
        );
    }

    #[test]
    fn thread_backed_recorder_keeps_full_event_history_beyond_thread_buffer() {
        let handle = ThreadManager::with_event_buffer_capacity(2)
            .start_thread_with_identifier("thread", ThreadBootstrap::new(None));
        let mut recorder = ExecEventRecorder::new("thread", None, Some(handle.clone()));

        recorder.turn_started();
        recorder.agent_message("first");
        recorder.agent_message("second");
        recorder.turn_completed();

        let full_events = recorder.into_events();
        let buffered_events = handle.recent_events();

        assert_eq!(buffered_events.len(), 2);
        assert!(full_events.len() > buffered_events.len());
        assert_eq!(
            full_events
                .iter()
                .filter(|event| matches!(event, ThreadEvent::ItemCompleted(_)))
                .count(),
            2
        );
    }
}
