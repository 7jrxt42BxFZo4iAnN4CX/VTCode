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

use std::path::PathBuf;

use anyhow::Context;
use thiserror::Error;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_ui::tui::app::InlineHandle;

use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;

// Keep vtcode-core's planning implementation behind this facade. Runloop
// modules should depend on these re-exports instead of reaching through the
// core tool-handler path directly.
pub(crate) use vtcode_core::tools::handlers::planning_workflow::{
    PlanValidationReport, PlanningWorkflowState, merge_plan_content, persist_plan_draft, tracker_file_for_plan_file,
    validate_plan_content,
};

pub(crate) async fn persisted_plan_is_ready(state: &PlanningWorkflowState) -> bool {
    let Some(plan_file) = state.get_plan_file().await else {
        return false;
    };
    let Ok(plan_text) = tokio::fs::read_to_string(&plan_file).await else {
        return false;
    };
    if !validate_plan_content(&plan_text).is_ready() {
        return false;
    }

    let Some(tracker_file) = tracker_file_for_plan_file(&plan_file) else {
        return false;
    };
    let Ok(tracker_text) = tokio::fs::read_to_string(&tracker_file).await else {
        return false;
    };
    if tracker_text.trim().is_empty() {
        return false;
    }

    match state.workspace_root() {
        Some(workspace_root) => {
            tokio::fs::read_to_string(workspace_root.join(".vtcode").join("tasks").join("current_task.md"))
                .await
                .ok()
                .is_some_and(|content| !content.trim().is_empty())
        }
        None => true,
    }
}

pub(crate) use super::planning_workflow_state::{PlanningFinishReason, finish_planning_workflow};
pub(crate) use confirmation::{StartPlanningDecision, execute_plan_approval, present_start_planning_confirmation};
pub(crate) use events::{emit_context_reset, emit_plan_approval_resolved, emit_plan_ready_events};
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
pub(crate) use task_tracker::{TaskTrackerHandoff, create_task_tracker_from_active_plan};

/// Build a one-shot repair directive from validator-owned feedback. The
/// feedback (produced by `PlanValidationReport::repair_feedback()`) is bounded
/// — no raw plan lines — and always includes the canonical step format so the
/// model knows the exact contract. The policy prose (one repair, no tool calls,
/// re-emit `<proposed_plan>`) is owned by this facade.
///
/// Both the initial-plan rejection path (`response_handling.rs`) and the
/// later-turn approval rejection path (`exit_trigger.rs`) use this helper so
/// the model receives consistent format guidance from every repair surface.
pub(crate) fn build_plan_repair_directive(feedback: &str) -> String {
    format!(
        "Planning recovery: the proposed plan was rejected. Repair it once using concrete repository evidence. \
         {feedback}\n\n\
         Re-emit exactly one compact `<proposed_plan>` with Summary, numbered Implementation Steps in the canonical \
         form, Test Cases and Validation, and Assumptions and Defaults. Resolve every open decision. Do not emit tool \
         calls or ask for approval until the artifact is complete."
    )
}

#[derive(Debug, Clone)]
pub(crate) struct ValidatedPlanArtifact {
    pub(crate) plan_file: PathBuf,
    pub(crate) text: String,
    pub(crate) validation: PlanValidationReport,
}

#[derive(Debug, Error)]
pub(crate) enum PlanArtifactError {
    #[error("the planning workflow has no persisted plan draft")]
    Missing,
    #[error("failed to read persisted plan {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("invalid plan artifact: {reasons}")]
    Invalid {
        reasons: String,
        // Boxed to keep `PlanArtifactError` under the `result_large_err`
        // threshold — the report carries four Vecs and is only needed on the
        // error path, not the hot path.
        report: Box<PlanValidationReport>,
    },
    #[error("failed to persist plan draft: {reason}")]
    Persistence { reason: String },
}

impl ValidatedPlanArtifact {
    pub(crate) fn from_text(plan_file: PathBuf, text: String) -> Result<Self, PlanArtifactError> {
        let validation = validate_plan_content(&text);
        if !validation.is_ready() {
            return Err(PlanArtifactError::Invalid {
                reasons: validation.reasons().join("; "),
                report: Box::new(validation),
            });
        }
        Ok(Self { plan_file, text, validation })
    }

    /// Construct a `ValidatedPlanArtifact` from an already-validated report,
    /// skipping a redundant revalidation of the same immutable text. The caller
    /// must guarantee `validation.is_ready()` (enforced by debug_assert in
    /// non-release builds and by the persistence layer in all builds).
    pub(crate) fn from_validated(plan_file: PathBuf, text: String, validation: PlanValidationReport) -> Self {
        debug_assert!(validation.is_ready(), "from_validated called with a non-ready validation report");
        Self { plan_file, text, validation }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ApprovedPlanHandoff {
    pub(crate) plan: ValidatedPlanArtifact,
    pub(crate) tracker: TaskTrackerHandoff,
    pub(crate) execution_agent: Option<String>,
    pub(crate) skip_confirmations: bool,
    pub(crate) execution_context: PlanExecutionContext,
}

pub(crate) async fn complete_approved_plan_handoff(
    tool_registry: &ToolRegistry,
    plan_session: &mut PlanningWorkflowSessionState,
    handle: &InlineHandle,
    plan: ValidatedPlanArtifact,
    active_agent_name: &str,
    skip_confirmations: bool,
    execution_context: PlanExecutionContext,
) -> anyhow::Result<ApprovedPlanHandoff> {
    let execution_agent = plan_session.execution_agent_after_approval(active_agent_name);
    let tracker = finish_planning_workflow(tool_registry, plan_session, handle, PlanningFinishReason::Approved)
        .await?
        .context("approved-plan handoff completed without a task tracker")?;
    handle.set_skip_confirmations(skip_confirmations);
    let handoff = ApprovedPlanHandoff {
        plan,
        tracker,
        execution_agent,
        skip_confirmations,
        execution_context,
    };
    tracing::info!(
        target: "vtcode.planning_workflow",
        plan_file = %handoff.plan.plan_file.display(),
        implementation_steps = handoff.plan.validation.implementation_step_count,
        tracker_plan_file = %handoff.tracker.plan_file.display(),
        tracker_file = %handoff.tracker.tracker_file.display(),
        tracker_items = handoff.tracker.item_count,
        skip_confirmations = handoff.skip_confirmations,
        execution_context = ?handoff.execution_context,
        "approved-plan handoff completed"
    );
    Ok(handoff)
}

/// Resolve the current approval request using its original telemetry identity.
/// The fallback IDs are used only for legacy callers that resolve an approval
/// before a request was recorded.
pub(crate) fn resolve_plan_approval(
    plan_session: &mut PlanningWorkflowSessionState,
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
