//! Planning-workflow facade.
//!
//! Single module boundary for the planning domain. The runloop must depend only
//! on this facade (the `pub(crate)` re-exports below), never on individual
//! submodule paths or on `vtcode-core`'s planning tool internals. Submodules are
//! `pub(crate)` internals so the domain stays cohesively isolated and each piece
//! remains independently testable (intent detection, HITL confirmation, tool
//! dispatch, truncation recovery).
//!
//! This is the interface guard rail for the next-generation planning refactor:
//! widening the public surface means editing the re-exports here, which makes
//! accidental cross-module coupling visible at review time.

pub(crate) mod confirmation;
pub(crate) mod events;
pub(crate) mod execution;
pub(crate) mod exit_trigger;
pub(crate) mod intent;
pub(crate) mod plan_approval;
pub(crate) mod recovery;
pub(crate) mod start_confirmation;
pub(crate) mod task_tracker;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PlanExecutionContext {
    #[default]
    Current,
    Fresh,
}

// --- Stable interface (the only planning symbols the runloop should name) ---

pub(crate) use super::planning_workflow_state::finish_planning_workflow;
pub(crate) use confirmation::{StartPlanningDecision, execute_plan_approval, present_start_planning_confirmation};
pub(crate) use events::{emit_context_reset, emit_plan_approval_requested, emit_plan_approval_resolved};
pub(crate) use execution::handle_start_planning;
pub(crate) use exit_trigger::{PlanningExitContext, maybe_handle_planning_exit_trigger};
pub(crate) use intent::{
    PlanningIntent, assistant_recently_prompted_implementation, detect_enter_planning_intent, detect_planning_intent,
};
pub(crate) use plan_approval::{
    PlanApprovalRequestContext, PlanApprovalRoute, PlanApprovalTelemetryContext, load_plan_text_for_approval,
    plan_approval_route,
};
pub(crate) use recovery::maybe_condense_truncated_plan;
pub(crate) use task_tracker::create_task_tracker_from_active_plan;

/// Resolve the current approval request using its original telemetry identity.
/// The fallback IDs are used only for legacy callers that resolve an approval
/// before a request was recorded.
pub(crate) fn resolve_plan_approval(
    plan_session: &mut super::planning_workflow_state::PlanningWorkflowSessionState,
    emitter: Option<&crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter>,
    fallback_thread_id: &str,
    fallback_turn_id: &str,
    decision: vtcode_core::exec::events::PlanApprovalDecision,
    automatic: bool,
) {
    let Some(pending) = plan_session.take_pending_plan_approval() else {
        return;
    };
    emit_plan_approval_resolved(
        emitter,
        if pending.thread_id.is_empty() {
            fallback_thread_id.to_owned()
        } else {
            pending.thread_id
        },
        if pending.turn_id.is_empty() {
            fallback_turn_id.to_owned()
        } else {
            pending.turn_id
        },
        decision,
        automatic,
    );
}
