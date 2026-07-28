use std::sync::Arc;

use tokio::sync::Notify;
use vtcode_config::VTCodeConfig;
use vtcode_core::exec::events::PlanApprovalDecision;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_ui::tui::app::{InlineHandle, InlineSession};

use crate::agent::runloop::unified::planning_workflow::{
    PlanApprovalRoute, PlanApprovalTelemetryContext, PlanningIntent, assistant_recently_prompted_implementation,
    detect_planning_intent, execute_plan_approval, load_plan_text_for_approval, plan_approval_route,
};
use crate::agent::runloop::unified::planning_workflow_state::{
    PlanningWorkflowSessionState, finish_planning_workflow, short_confirmation_hint_with_fallback,
};
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnLoopResult};

const PLANNING_WORKFLOW_EXIT_TRIGGER_STATUS: &str = "Planning workflow: implementation intent detected from your message. Exiting planning mode and proceeding with execution.";
const PLANNING_WORKFLOW_MISSING_PLAN_SYNTHESIS_DIRECTIVE: &str = "Planning recovery: implementation was requested, but no completed plan draft exists yet. Do not implement and do not ask for approval. Synthesize exactly one compact `<proposed_plan>` from the repository evidence already gathered, including Summary, numbered `Action -> files/symbols -> verify:` steps, Validation, and short Assumptions. Do not emit tool calls.";

pub(crate) struct PlanningExitContext<'a> {
    pub(crate) active_agent_name: &'a str,
    pub(crate) session: &'a mut InlineSession,
    pub(crate) ctrl_c_state: &'a Arc<CtrlCState>,
    pub(crate) ctrl_c_notify: &'a Arc<Notify>,
    pub(crate) vt_cfg: Option<&'a VTCodeConfig>,
    pub(crate) skip_confirmations: bool,
    pub(crate) full_auto: bool,
    pub(crate) telemetry: PlanApprovalTelemetryContext<'a>,
}

/// Outcome of checking whether the planning workflow should exit this turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanningTransition {
    /// No planning transition; continue the turn normally.
    None,
    /// User approved the plan; proceed with execution.
    ExitAndImplement {
        execution_agent: Option<String>,
        auto_accept: bool,
    },
    /// User wants to stay in planning mode.
    StayInPlanning,
    /// User abandoned the current plan without starting execution.
    CancelPlanning,
}

impl PlanningTransition {
    /// Convert this transition into the `TurnLoopResult::Completed` variant
    /// and an optional primary-agent switch command.
    #[inline]
    pub(crate) fn into_result_and_agent(self) -> (TurnLoopResult, Option<String>, bool) {
        match self {
            PlanningTransition::None => {
                (TurnLoopResult::Completed { plan_approved_execution_pending: false }, None, false)
            }
            PlanningTransition::ExitAndImplement { execution_agent, auto_accept } => {
                (TurnLoopResult::Completed { plan_approved_execution_pending: true }, execution_agent, auto_accept)
            }
            PlanningTransition::StayInPlanning => {
                (TurnLoopResult::Completed { plan_approved_execution_pending: false }, None, false)
            }
            PlanningTransition::CancelPlanning => {
                (TurnLoopResult::Completed { plan_approved_execution_pending: false }, None, false)
            }
        }
    }

    /// Whether the turn loop should break after this transition.
    #[inline]
    pub(crate) fn should_break(&self) -> bool {
        !matches!(self, PlanningTransition::None)
    }
}

/// Check whether the last user message signals a planning-workflow exit (approve,
/// implement, switch-to-build/auto) and execute the transition if so.
///
/// Returns the detected transition. The caller checks `should_break()` to decide
/// whether to break the turn loop.
pub(crate) async fn maybe_handle_planning_exit_trigger(
    renderer: &mut AnsiRenderer,
    tool_registry: &mut ToolRegistry,
    plan_session: &mut PlanningWorkflowSessionState,
    handle: &InlineHandle,
    working_history: &mut Vec<uni::Message>,
    auto_finish_planning_attempted: &mut bool,
    exit_context: PlanningExitContext<'_>,
) -> anyhow::Result<PlanningTransition> {
    if !tool_registry.is_planning_active() {
        return Ok(PlanningTransition::None);
    }

    if *auto_finish_planning_attempted {
        return Ok(PlanningTransition::None);
    }

    let Some(last_user_msg) = working_history.iter().rev().find(|msg| msg.role == uni::MessageRole::User) else {
        return Ok(PlanningTransition::None);
    };

    let text = last_user_msg.content.as_text();
    let assistant_prompted = assistant_recently_prompted_implementation(working_history);
    let intent = detect_planning_intent(&text, assistant_prompted);

    let transition = match intent {
        PlanningIntent::ExitAndImplement => {
            let Some(plan_text) = load_plan_text_for_approval(tool_registry).await else {
                display_status(
                    renderer,
                    "No completed plan draft exists yet. I will synthesize the plan from the gathered evidence before showing approval.",
                )?;
                // A textual `yes`/`implement` is not an approval when there
                // is no persisted draft. Keep the user choice attached to
                // this turn, but continue through one model request so the
                // plan can be synthesized and then routed to the normal
                // approval overlay. The turn-local guard prevents the same
                // user message from re-entering this branch on the next loop
                // iteration.
                *auto_finish_planning_attempted = true;
                working_history
                    .push(uni::Message::system(PLANNING_WORKFLOW_MISSING_PLAN_SYNTHESIS_DIRECTIVE.to_string()));
                return Ok(PlanningTransition::None);
            };

            *auto_finish_planning_attempted = true;

            let require_confirmation = exit_context
                .vt_cfg
                .map(|cfg| cfg.agent.require_plan_confirmation)
                .unwrap_or(true);
            let approval_route = plan_approval_route(
                require_confirmation,
                renderer.supports_inline_ui(),
                exit_context.skip_confirmations,
                exit_context.full_auto,
            );
            tracing::info!(
                target: "vtcode.planning_workflow",
                ?approval_route,
                "textual plan approval requested"
            );

            if approval_route == PlanApprovalRoute::Inline {
                let outcome = execute_plan_approval(
                    tool_registry,
                    plan_session,
                    handle,
                    exit_context.session,
                    exit_context.ctrl_c_state,
                    exit_context.ctrl_c_notify,
                    &plan_text,
                    exit_context.active_agent_name,
                    exit_context.telemetry,
                )
                .await?;

                return Ok(match outcome {
                    TurnHandlerOutcome::SwitchPrimaryAgent(execution_agent) => PlanningTransition::ExitAndImplement {
                        execution_agent: Some(execution_agent),
                        auto_accept: false,
                    },
                    TurnHandlerOutcome::SwitchPrimaryAgentWithPolicy { agent, skip_confirmations } => {
                        PlanningTransition::ExitAndImplement {
                            execution_agent: Some(agent),
                            auto_accept: skip_confirmations,
                        }
                    }
                    TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: true }) => {
                        PlanningTransition::ExitAndImplement { execution_agent: None, auto_accept: false }
                    }
                    TurnHandlerOutcome::BreakWithPolicy {
                        result: TurnLoopResult::Completed { plan_approved_execution_pending: true },
                        skip_confirmations,
                    } => PlanningTransition::ExitAndImplement {
                        execution_agent: None,
                        auto_accept: skip_confirmations,
                    },
                    TurnHandlerOutcome::Break(_) | TurnHandlerOutcome::Continue => PlanningTransition::StayInPlanning,
                    TurnHandlerOutcome::BreakWithPolicy { .. } => PlanningTransition::StayInPlanning,
                });
            }

            display_status(renderer, PLANNING_WORKFLOW_EXIT_TRIGGER_STATUS)?;
            let execution_agent = plan_session.execution_agent_after_approval(exit_context.active_agent_name);
            let decision = if approval_route == PlanApprovalRoute::Automatic {
                PlanApprovalDecision::AutoAccept
            } else {
                PlanApprovalDecision::Execute
            };
            super::resolve_plan_approval(
                plan_session,
                exit_context.telemetry.emitter,
                exit_context.telemetry.thread_id,
                exit_context.telemetry.turn_id,
                decision,
                approval_route == PlanApprovalRoute::Automatic,
            );
            handle.set_skip_confirmations(approval_route == PlanApprovalRoute::Automatic);
            finish_planning_workflow(tool_registry, plan_session, handle, false).await;
            PlanningTransition::ExitAndImplement {
                execution_agent,
                auto_accept: approval_route == PlanApprovalRoute::Automatic,
            }
        }
        PlanningIntent::StayInPlanning => {
            display_status(renderer, &short_confirmation_hint_with_fallback())?;
            super::resolve_plan_approval(
                plan_session,
                exit_context.telemetry.emitter,
                exit_context.telemetry.thread_id,
                exit_context.telemetry.turn_id,
                PlanApprovalDecision::Revise,
                false,
            );
            PlanningTransition::StayInPlanning
        }
        PlanningIntent::CancelPlanning => {
            display_status(renderer, "Planning workflow cancelled; the plan was not implemented.")?;
            super::resolve_plan_approval(
                plan_session,
                exit_context.telemetry.emitter,
                exit_context.telemetry.thread_id,
                exit_context.telemetry.turn_id,
                PlanApprovalDecision::Cancel,
                false,
            );
            finish_planning_workflow(tool_registry, plan_session, handle, true).await;
            PlanningTransition::CancelPlanning
        }
        PlanningIntent::None => PlanningTransition::None,
    };

    Ok(transition)
}

fn display_status(renderer: &mut AnsiRenderer, message: &str) -> anyhow::Result<()> {
    renderer.line(vtcode_core::utils::ansi::MessageStyle::Status, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approved_plan_transition_preserves_auto_accept_for_agent_handoff() {
        let (result, agent, auto_accept) = PlanningTransition::ExitAndImplement {
            execution_agent: Some("build".to_string()),
            auto_accept: true,
        }
        .into_result_and_agent();

        assert!(matches!(result, TurnLoopResult::Completed { plan_approved_execution_pending: true }));
        assert_eq!(agent.as_deref(), Some("build"));
        assert!(auto_accept);
    }

    #[test]
    fn manual_plan_transition_keeps_confirmation_prompts_without_agent_switch() {
        let (result, agent, auto_accept) =
            PlanningTransition::ExitAndImplement { execution_agent: None, auto_accept: false }.into_result_and_agent();

        assert!(matches!(result, TurnLoopResult::Completed { plan_approved_execution_pending: true }));
        assert!(agent.is_none());
        assert!(!auto_accept);
    }
}
