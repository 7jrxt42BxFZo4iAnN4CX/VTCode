//! Shared planning approval events for interactive and headless runtimes.

use vtcode_core::exec::events::{
    PlanApprovalDecision, PlanApprovalRequestedEvent, PlanApprovalResolvedEvent, ThreadEvent,
};

use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;

pub(crate) fn emit_plan_approval_requested(
    emitter: Option<&HarnessEventEmitter>,
    thread_id: impl Into<String>,
    turn_id: impl Into<String>,
    plan_file: Option<String>,
) {
    let Some(emitter) = emitter else {
        return;
    };

    if let Err(err) = emitter.emit(ThreadEvent::PlanApprovalRequested(PlanApprovalRequestedEvent {
        thread_id: thread_id.into(),
        turn_id: turn_id.into(),
        plan_file,
    })) {
        tracing::debug!(error = %err, "failed to emit plan approval request event");
    }
}

pub(crate) fn emit_plan_approval_resolved(
    emitter: Option<&HarnessEventEmitter>,
    thread_id: impl Into<String>,
    turn_id: impl Into<String>,
    decision: PlanApprovalDecision,
    automatic: bool,
) {
    let Some(emitter) = emitter else {
        return;
    };

    if let Err(err) = emitter.emit(ThreadEvent::PlanApprovalResolved(PlanApprovalResolvedEvent {
        thread_id: thread_id.into(),
        turn_id: turn_id.into(),
        decision,
        automatic,
    })) {
        tracing::debug!(error = %err, "failed to emit plan approval resolution event");
    }
}

#[cfg(test)]
mod tests {
    use super::{emit_plan_approval_requested, emit_plan_approval_resolved};
    use std::path::PathBuf;
    use tempfile::tempdir;
    use vtcode_core::exec::events::{PlanApprovalDecision, ThreadEvent, VersionedThreadEvent};

    #[test]
    fn approval_events_are_written_to_the_shared_harness_stream() {
        let directory = tempdir().expect("temporary event directory");
        let path = directory.path().join(PathBuf::from("events.jsonl"));
        let emitter = super::HarnessEventEmitter::new(path.clone()).expect("harness emitter");

        emit_plan_approval_requested(Some(&emitter), "thread-1", "turn-1", Some(".vtcode/plans/task.md".to_string()));
        emit_plan_approval_resolved(Some(&emitter), "thread-1", "turn-2", PlanApprovalDecision::SwitchAuto, false);

        let lines = std::fs::read_to_string(path)
            .expect("event log")
            .lines()
            .map(|line| serde_json::from_str::<VersionedThreadEvent>(line).expect("versioned event"))
            .map(VersionedThreadEvent::into_event)
            .collect::<Vec<_>>();

        assert!(matches!(lines[0], ThreadEvent::PlanApprovalRequested(_)));
        assert!(matches!(
            lines[1],
            ThreadEvent::PlanApprovalResolved(ref event)
                if event.decision == PlanApprovalDecision::SwitchAuto && !event.automatic
        ));
    }
}
