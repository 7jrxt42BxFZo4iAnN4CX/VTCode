//! Plan-approval HITL flow: "Ready to code?" overlay + plan rendering.
//!
//! Isolates the plan-confirmation UI from the start-planning entry flow and from
//! the runloop's response handling. The `execute_plan_confirmation` function is
//! the canonical interface; callers map the returned `PlanConfirmationOutcome` to
//! their own transition logic.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Notify;
use vtcode_core::exec::events::PlanApprovalDecision;
use vtcode_ui::tui::app::{
    InlineHandle, InlineListItem, InlineListSelection, InlineMessageKind, InlineSession, ListOverlayRequest,
    PlanContent, TransientHotkey, TransientHotkeyAction, TransientHotkeyKey, TransientRequest, TransientSubmission,
};

use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_overlay_and_wait};
use crate::agent::runloop::unified::state::CtrlCState;

/// Result of the plan confirmation flow
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanConfirmationOutcome {
    /// User approved execution with manual edit approvals
    Execute,
    /// User approved with auto-accept enabled for future confirmations
    AutoAccept,
    /// User wants to edit the plan
    EditPlan,
    /// User cancelled
    Cancel,
    /// User chose to hand off execution to the build primary agent.
    SwitchBuild,
    /// User chose to hand off execution to the auto primary agent.
    SwitchAuto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanApprovalRoute {
    Inline,
    Headless,
    Automatic,
}

pub(crate) fn plan_approval_route(
    require_confirmation: bool,
    supports_inline_ui: bool,
    skip_confirmations: bool,
    full_auto: bool,
) -> PlanApprovalRoute {
    if require_confirmation && !skip_confirmations && !full_auto {
        if supports_inline_ui {
            PlanApprovalRoute::Inline
        } else {
            PlanApprovalRoute::Headless
        }
    } else {
        PlanApprovalRoute::Automatic
    }
}

/// Event identity carried through the approval overlay so all outcomes are
/// recorded against the same planning turn without widening the function API.
pub(crate) struct PlanApprovalTelemetryContext<'a> {
    pub(crate) emitter: Option<&'a HarnessEventEmitter>,
    pub(crate) thread_id: &'a str,
    pub(crate) turn_id: &'a str,
}

fn line_count(text: &str) -> usize {
    text.lines().count().max(1)
}

fn append_message(handle: &InlineHandle, kind: InlineMessageKind, text: impl Into<String>) {
    let text = text.into();
    handle.append_pasted_message(kind, text.clone(), line_count(&text));
}

fn render_confirmation_prompt(handle: &InlineHandle, plan: &PlanContent) {
    tracing::info!(
        target: "vtcode.planning_workflow",
        plan_title = %plan.title,
        "render_confirmation_prompt: adding confirmation messages"
    );
    append_message(handle, InlineMessageKind::Info, "Ready to code?");
    append_message(handle, InlineMessageKind::Info, "A plan is ready to execute. Would you like to proceed?");

    if !plan.summary.trim().is_empty() {
        append_message(handle, InlineMessageKind::Agent, plan.summary.clone());
    } else if !plan.title.trim().is_empty() {
        append_message(handle, InlineMessageKind::Info, format!("Plan: {}", plan.title));
    }

    // Keep the approval overlay compact, but put the complete persisted draft
    // in the scrollable transcript so users can review every section before
    // choosing an execution mode. The overlay's `lines` field is intentionally
    // bounded by `render_plan_summary` and must not be used for this content.
    if !plan.raw_content.trim().is_empty() {
        append_message(handle, InlineMessageKind::Info, "Implementation plan (full):");
        append_message(handle, InlineMessageKind::Agent, plan.raw_content.clone());
    }

    if let Some(path) = plan.file_path.as_deref()
        && !path.trim().is_empty()
    {
        append_message(handle, InlineMessageKind::Info, format!("Plan file: {path}"));
    }
    append_message(
        handle,
        InlineMessageKind::Info,
        "Use the confirmation list to choose auto-accept, manual approve, or revise.",
    );
}

/// Render a robust, structured summary of the plan for the confirmation overlay.
///
/// Prefers the parsed `phases`/`steps` shape so the plan reads as a clear,
/// scannable checklist. Falls back to the raw content or summary when the
/// structured data is absent, so a malformed or partially synthesized plan
/// still renders something useful instead of a blank panel.
#[cfg(test)]
pub(crate) fn render_structured_plan(plan: &PlanContent) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();

    if !plan.title.trim().is_empty() {
        lines.push(plan.title.trim().to_string());
        lines.push(String::new());
    }

    if !plan.summary.trim().is_empty() {
        for line in plan.summary.trim().lines() {
            lines.push(line.to_string());
        }
        lines.push(String::new());
    }

    let has_phases = plan.phases.iter().any(|phase| !phase.steps.is_empty());
    if has_phases {
        for phase in &plan.phases {
            if phase.steps.is_empty() {
                continue;
            }
            if !phase.name.trim().is_empty() {
                lines.push(format!("## {}", phase.name.trim()));
            }
            for step in &phase.steps {
                let marker = if step.completed { "[x]" } else { "[ ]" };
                lines.push(format!("{} {} {}", marker, step.number, step.description));
                if let Some(details) = step.details.as_ref().filter(|d| !d.trim().is_empty()) {
                    for detail_line in details.lines() {
                        lines.push(format!("      {detail_line}"));
                    }
                }
                if !step.files.is_empty() {
                    lines.push(format!("      files: {}", step.files.join(", ")));
                }
            }
            lines.push(String::new());
        }
    } else if !plan.raw_content.trim().is_empty() {
        for line in plan.raw_content.lines() {
            lines.push(line.to_string());
        }
        lines.push(String::new());
    }

    if !plan.open_questions.is_empty() {
        lines.push("Open questions:".to_string());
        for question in &plan.open_questions {
            lines.push(format!("- {question}"));
        }
        lines.push(String::new());
    }

    if lines.is_empty() {
        lines.push(plan.title.trim().to_string());
    }

    lines
}

// The confirmation prompt consumes one of the modal's six instruction rows.
const PLAN_PREVIEW_MAX_LINES: usize = 5;
const PLAN_PREVIEW_MAX_CHARS: usize = 64;

fn truncate_plan_preview(text: &str) -> String {
    let text = text.trim();
    let mut chars = text.chars();
    let preview: String = chars.by_ref().take(PLAN_PREVIEW_MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{preview}…")
    } else {
        preview
    }
}

fn numbered_step_description(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let (number, description) = trimmed.split_once('.')?;
    if number.is_empty() || !number.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }

    let description = description.trim();
    (!description.is_empty()).then(|| format!("{number}. {description}"))
}

/// Render a bounded, decision-ready plan synopsis for the inline approval UI.
///
/// The plan file and plan events retain the complete markdown. The approval
/// modal has a small instruction viewport, so it receives only the summary and
/// numbered steps, with long lines elided and an explicit count for omitted
/// steps.
pub(crate) fn render_plan_summary(plan: &PlanContent) -> Vec<String> {
    let mut lines = Vec::new();
    if !plan.summary.trim().is_empty() {
        lines.push(format!("Summary: {}", truncate_plan_preview(&plan.summary)));
    }

    let structured_steps: Vec<String> = plan
        .phases
        .iter()
        .flat_map(|phase| phase.steps.iter())
        .map(|step| format!("{}. {}", step.number, step.description))
        .collect();
    let step_lines = if structured_steps.is_empty() {
        plan.raw_content.lines().filter_map(numbered_step_description).collect()
    } else {
        structured_steps
    };

    let available_step_lines = PLAN_PREVIEW_MAX_LINES.saturating_sub(lines.len());
    if step_lines.len() > available_step_lines {
        let visible_step_count = available_step_lines.saturating_sub(1);
        lines.extend(
            step_lines
                .iter()
                .take(visible_step_count)
                .map(|line| truncate_plan_preview(line)),
        );
        lines.push(format!("… and {} more plan steps", step_lines.len() - visible_step_count));
    } else {
        lines.extend(step_lines.iter().map(|line| truncate_plan_preview(line)));
    }

    if lines.is_empty() {
        let fallback = if plan.title.trim().is_empty() {
            "Plan details are available in the plan file.".to_string()
        } else {
            format!("Plan: {}", plan.title.trim())
        };
        lines.push(truncate_plan_preview(&fallback));
    }

    lines
}

pub(crate) fn build_plan_confirmation_request(plan: &PlanContent, draft_incomplete: bool) -> TransientRequest {
    tracing::info!(
        target: "vtcode.planning_workflow",
        draft_incomplete,
        "build_plan_confirmation_request: building overlay request"
    );
    let mut lines = render_plan_summary(plan);
    lines.insert(0, "A plan is ready to execute. Would you like to proceed?".to_string());

    let footer_hint = plan
        .file_path
        .as_ref()
        .map(|path| format!("ctrl-g to edit in VS Code · {path}"));
    let items = vec![
        InlineListItem {
            title: "Yes, auto-accept edits".to_string(),
            subtitle: Some("Execute with auto-approval.".to_string()),
            badge: Some("Recommended".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalAutoAccept),
            search_value: None,
        },
        InlineListItem {
            title: "Yes, manually approve edits".to_string(),
            subtitle: Some("Keep context and confirm each edit before applying.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalExecute),
            search_value: None,
        },
        InlineListItem {
            title: "Type feedback to revise the plan".to_string(),
            subtitle: Some("Return to planning workflow and refine the plan.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalEditPlan),
            search_value: None,
        },
        InlineListItem {
            title: "Switch to build agent".to_string(),
            subtitle: Some("Hand off to the build agent to execute the plan with manual edit approvals.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalSwitchBuild),
            search_value: None,
        },
        InlineListItem {
            title: "Switch to auto agent".to_string(),
            subtitle: Some(
                "Hand off to the auto agent to auto-execute the plan (skip per-step confirmations).".to_string(),
            ),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalSwitchAuto),
            search_value: None,
        },
    ];

    let selected = if draft_incomplete {
        InlineListSelection::PlanApprovalEditPlan
    } else {
        InlineListSelection::PlanApprovalAutoAccept
    };

    TransientRequest::List(ListOverlayRequest {
        title: "Ready to code?".to_string(),
        lines,
        footer_hint,
        items,
        selected: Some(selected),
        search: None,
        hotkeys: vec![TransientHotkey {
            key: TransientHotkeyKey::CtrlChar('g'),
            action: TransientHotkeyAction::LaunchEditor,
        }],
    })
}

/// Map a plan-confirmation overlay submission to its [`PlanConfirmationOutcome`].
///
/// Kept as a pure function so the selection→outcome mapping (including the
/// `SwitchBuild`/`SwitchAuto` handoff outcomes) is unit-testable without the
/// TUI driver. The `LaunchEditor` hotkey maps to `EditPlan`; any other
/// unrecognized selection cancels.
pub(crate) fn plan_confirmation_submission_to_outcome(
    submission: &TransientSubmission,
) -> Option<PlanConfirmationOutcome> {
    match submission {
        TransientSubmission::Selection(InlineListSelection::PlanApprovalExecute) => {
            Some(PlanConfirmationOutcome::Execute)
        }
        TransientSubmission::Selection(InlineListSelection::PlanApprovalAutoAccept) => {
            Some(PlanConfirmationOutcome::AutoAccept)
        }
        TransientSubmission::Selection(InlineListSelection::PlanApprovalEditPlan) => {
            Some(PlanConfirmationOutcome::EditPlan)
        }
        TransientSubmission::Selection(InlineListSelection::PlanApprovalSwitchBuild) => {
            Some(PlanConfirmationOutcome::SwitchBuild)
        }
        TransientSubmission::Selection(InlineListSelection::PlanApprovalSwitchAuto) => {
            Some(PlanConfirmationOutcome::SwitchAuto)
        }
        TransientSubmission::Hotkey(TransientHotkeyAction::LaunchEditor) => Some(PlanConfirmationOutcome::EditPlan),
        TransientSubmission::Selection(_) => Some(PlanConfirmationOutcome::Cancel),
        _ => None,
    }
}

/// Execute the plan confirmation HITL flow.
///
/// The plan is rendered as static transcript markdown plus an inline confirmation list.
pub(crate) async fn execute_plan_confirmation(
    handle: &InlineHandle,
    session: &mut InlineSession,
    plan_content: PlanContent,
    draft_incomplete: bool,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<PlanConfirmationOutcome> {
    tracing::info!(
        target: "vtcode.planning_workflow",
        "execute_plan_confirmation: rendering confirmation prompt and showing overlay"
    );
    render_confirmation_prompt(handle, &plan_content);
    let outcome = show_overlay_and_wait(
        handle,
        session,
        build_plan_confirmation_request(&plan_content, draft_incomplete),
        ctrl_c_state,
        ctrl_c_notify,
        |submission| {
            if let TransientSubmission::Hotkey(TransientHotkeyAction::LaunchEditor) = submission {
                handle.set_input("/edit".to_string());
            }
            plan_confirmation_submission_to_outcome(&submission)
        },
    )
    .await?;

    tracing::info!(
        target: "vtcode.planning_workflow",
        overlay_wait_outcome = "done",
        "execute_plan_confirmation: overlay wait completed"
    );

    Ok(match outcome {
        OverlayWaitOutcome::Submitted(outcome) => outcome,
        OverlayWaitOutcome::Cancelled | OverlayWaitOutcome::Interrupted | OverlayWaitOutcome::Exit => {
            PlanConfirmationOutcome::Cancel
        }
    })
}

/// Load the persisted plan draft for an approval request received on a later
/// turn, such as a textual `approve` message. The initial plan turn passes its
/// draft directly through `execute_plan_approval`; subsequent turns must use
/// the persisted copy so they can present the same popup instead of bypassing
/// confirmation.
pub(crate) async fn load_plan_text_for_approval(tool_registry: &ToolRegistry) -> Option<String> {
    let plan_file = tool_registry.planning_workflow_state().get_plan_file().await?;
    tokio::fs::read_to_string(plan_file)
        .await
        .ok()
        .filter(|text| !text.trim().is_empty())
}

use crate::agent::runloop::unified::planning_workflow_state::{PlanningWorkflowSessionState, finish_planning_workflow};
use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnLoopResult};
use vtcode_config::{builtin_primary_auto_agent, builtin_primary_build_agent};
use vtcode_core::tools::registry::ToolRegistry;

/// Execute the plan-approval overlay and return the corresponding turn outcome.
///
/// This is the canonical interface for the inline plan-confirmation UI. It
/// renders the plan, shows the confirmation overlay, and maps the user's
/// choice onto the appropriate `TurnHandlerOutcome` (break/switch/continue).
///
/// Separated from `TurnProcessingContext` so the planning module owns the
/// approval flow and callers only provide the minimal dependencies.
pub(crate) async fn execute_plan_approval(
    tool_registry: &mut ToolRegistry,
    plan_session: &mut PlanningWorkflowSessionState,
    handle: &InlineHandle,
    session: &mut InlineSession,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    plan_text: &str,
    active_agent_name: &str,
    telemetry: PlanApprovalTelemetryContext<'_>,
) -> Result<TurnHandlerOutcome> {
    tracing::info!(
        target: "vtcode.planning_workflow",
        "execute_plan_approval: showing confirmation dialog"
    );

    let plan_file = tool_registry
        .planning_workflow_state()
        .get_plan_file()
        .await
        .map(|path| path.to_string_lossy().into_owned());
    let plan_content = PlanContent::from_markdown("Implementation Plan".to_string(), plan_text, plan_file);
    let outcome = execute_plan_confirmation(handle, session, plan_content, false, ctrl_c_state, ctrl_c_notify).await;

    tracing::info!(
        target: "vtcode.planning_workflow",
        overlay_outcome = ?outcome.as_ref().ok(),
        "execute_plan_approval: dialog closed"
    );

    let (decision, automatic) = match outcome.as_ref() {
        Ok(PlanConfirmationOutcome::Execute) => (PlanApprovalDecision::Execute, false),
        Ok(PlanConfirmationOutcome::AutoAccept) => (PlanApprovalDecision::AutoAccept, false),
        Ok(PlanConfirmationOutcome::EditPlan) => (PlanApprovalDecision::Revise, false),
        Ok(PlanConfirmationOutcome::SwitchBuild) => (PlanApprovalDecision::SwitchBuild, false),
        Ok(PlanConfirmationOutcome::SwitchAuto) => (PlanApprovalDecision::SwitchAuto, false),
        Ok(PlanConfirmationOutcome::Cancel) | Err(_) => (PlanApprovalDecision::Cancel, false),
    };
    super::resolve_plan_approval(
        plan_session,
        telemetry.emitter,
        telemetry.thread_id,
        telemetry.turn_id,
        decision,
        automatic,
    );

    match outcome {
        Ok(PlanConfirmationOutcome::Execute) => {
            let execution_agent = plan_session.execution_agent_after_approval(active_agent_name);
            finish_planning_workflow(tool_registry, plan_session, handle, false).await;
            handle.set_skip_confirmations(false);
            tracing::info!(
                target: "vtcode.planning_workflow",
                "User approved plan via inline overlay (manual edit approvals); enabling mutating tools"
            );
            Ok(execution_agent.map_or(
                TurnHandlerOutcome::BreakWithPolicy {
                    result: TurnLoopResult::Completed { plan_approved_execution_pending: true },
                    skip_confirmations: false,
                },
                |agent| TurnHandlerOutcome::SwitchPrimaryAgentWithPolicy { agent, skip_confirmations: false },
            ))
        }
        Ok(PlanConfirmationOutcome::AutoAccept) => {
            let execution_agent = plan_session.execution_agent_after_approval(active_agent_name);
            finish_planning_workflow(tool_registry, plan_session, handle, false).await;
            handle.set_skip_confirmations(true);
            tracing::info!(
                target: "vtcode.planning_workflow",
                "User approved plan via inline overlay (auto-accept); enabling mutating tools"
            );
            Ok(execution_agent.map_or(
                TurnHandlerOutcome::BreakWithPolicy {
                    result: TurnLoopResult::Completed { plan_approved_execution_pending: true },
                    skip_confirmations: true,
                },
                |agent| TurnHandlerOutcome::SwitchPrimaryAgentWithPolicy { agent, skip_confirmations: true },
            ))
        }
        Ok(PlanConfirmationOutcome::SwitchBuild) => {
            finish_planning_workflow(tool_registry, plan_session, handle, false).await;
            handle.set_skip_confirmations(false);
            tracing::info!(
                target: "vtcode.planning_workflow",
                "User handed plan to build agent via inline overlay; switching primary agent"
            );
            Ok(TurnHandlerOutcome::SwitchPrimaryAgentWithPolicy {
                agent: builtin_primary_build_agent().name,
                skip_confirmations: false,
            })
        }
        Ok(PlanConfirmationOutcome::SwitchAuto) => {
            finish_planning_workflow(tool_registry, plan_session, handle, false).await;
            handle.set_skip_confirmations(true);
            tracing::info!(
                target: "vtcode.planning_workflow",
                "User handed plan to auto agent via inline overlay; switching primary agent"
            );
            Ok(TurnHandlerOutcome::SwitchPrimaryAgentWithPolicy {
                agent: builtin_primary_auto_agent().name,
                skip_confirmations: true,
            })
        }
        Ok(PlanConfirmationOutcome::EditPlan) => {
            tracing::info!(
                target: "vtcode.planning_workflow",
                "User chose to revise the plan via inline overlay; remaining in Planning workflow"
            );
            Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: false }))
        }
        Ok(PlanConfirmationOutcome::Cancel) | Err(_) => {
            tracing::info!(
                target: "vtcode.planning_workflow",
                "User dismissed the plan via inline overlay; remaining in Planning workflow"
            );
            Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: false }))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::{Notify, mpsc};

    use super::{
        PlanApprovalRoute, PlanConfirmationOutcome, build_plan_confirmation_request, execute_plan_confirmation,
        plan_approval_route, plan_confirmation_submission_to_outcome, render_plan_summary, render_structured_plan,
    };
    use crate::agent::runloop::unified::state::CtrlCState;
    use vtcode_ui::tui::app::{
        InlineCommand, InlineEvent, InlineHandle, InlineListSelection, InlineMessageKind, InlineSession,
        ListOverlayRequest, TransientEvent, TransientHotkeyAction, TransientRequest, TransientSubmission,
    };
    use vtcode_ui::tui::app::{PlanContent, PlanPhase, PlanStep};

    fn sample_plan() -> PlanContent {
        PlanContent {
            title: "Add retry to synthesize".to_string(),
            summary: "Make plan-mode synthesize resilient to transient errors.".to_string(),
            file_path: Some("docs/plan.md".to_string()),
            phases: vec![PlanPhase {
                name: "Phase 1: Resilience".to_string(),
                completed: false,
                steps: vec![
                    PlanStep {
                        number: 1,
                        description: "Wrap generate in retry".to_string(),
                        details: Some("Use RetryPolicy::default()".to_string()),
                        files: vec!["src/a.rs".to_string()],
                        completed: false,
                    },
                    PlanStep {
                        number: 2,
                        description: "Add tests".to_string(),
                        details: None,
                        files: vec![],
                        completed: false,
                    },
                ],
            }],
            open_questions: vec!["Should we cap retries?".to_string()],
            raw_content: "RAW fallback content".to_string(),
            total_steps: 2,
            completed_steps: 0,
        }
    }

    // --- B: render_structured_plan ----------------------------------------

    #[test]
    fn approval_route_separates_inline_headless_and_automatic_policies() {
        assert_eq!(plan_approval_route(true, true, false, false), PlanApprovalRoute::Inline);
        assert_eq!(plan_approval_route(true, false, false, false), PlanApprovalRoute::Headless);
        assert_eq!(plan_approval_route(true, false, true, false), PlanApprovalRoute::Automatic);
        assert_eq!(plan_approval_route(false, true, false, false), PlanApprovalRoute::Automatic);
    }

    #[test]
    fn render_structured_plan_prefers_phases_over_raw() {
        let lines = render_structured_plan(&sample_plan());
        let joined = lines.join("\n");
        assert!(joined.contains("## Phase 1: Resilience"));
        assert!(joined.contains("[ ] 1 Wrap generate in retry"));
        assert!(joined.contains("Use RetryPolicy::default()"));
        assert!(joined.contains("files: src/a.rs"));
        assert!(joined.contains("Open questions:"));
        assert!(joined.contains("Should we cap retries?"));
        assert!(!joined.contains("RAW fallback content"), "structured phases must take precedence over raw_content");
    }

    #[test]
    fn render_structured_plan_falls_back_to_raw_content() {
        let mut plan = sample_plan();
        plan.phases = vec![];
        let lines = render_structured_plan(&plan);
        let joined = lines.join("\n");
        assert!(joined.contains("RAW fallback content"));
        assert!(joined.contains("Make plan-mode synthesize resilient"));
        assert!(!joined.contains("## Phase 1"));
    }

    #[test]
    fn render_structured_plan_falls_back_to_title_when_empty() {
        let plan = PlanContent {
            title: "Only a title".to_string(),
            summary: String::new(),
            file_path: None,
            phases: vec![],
            open_questions: vec![],
            raw_content: String::new(),
            total_steps: 0,
            completed_steps: 0,
        };
        assert_eq!(render_structured_plan(&plan), vec!["Only a title".to_string(), String::new()]);
    }

    #[test]
    fn plan_summary_preview_keeps_sparse_markdown_decision_complete() {
        let plan = PlanContent::from_markdown(
            "Implementation Plan".to_string(),
            "Summary\nFocus the launch-time improvement on startup latency.\n\n1. Instrument startup timing -> src/startup.rs -> verify: add logs.\n2. Move refresh off the critical path -> src/update.rs -> verify: UI renders first.\n3. Prefer the cached notice -> src/update.rs -> verify: stale refresh is deferred.\n4. Add a bounded fallback -> src/startup.rs -> verify: startup never waits.\n5. Validate with focused tests -> src/startup.rs -> verify: nextest passes.\n\nValidation\n- cargo check --locked",
            Some(".vtcode/plans/startup.md".to_string()),
        );

        let lines = render_plan_summary(&plan);
        assert_eq!(plan.summary, "Focus the launch-time improvement on startup latency.");
        assert_eq!(lines.len(), 5);
        assert!(lines[0].starts_with("Summary: Focus the launch-time improvement"));
        assert!(lines.iter().any(|line| line.starts_with("1. Instrument startup timing")));
        assert!(lines.iter().any(|line| line == "… and 2 more plan steps"));
        assert!(!lines.iter().any(|line| line == "Summary"));
    }

    // --- C: switch outcomes (submission mapping, request items) ------

    #[test]
    fn plan_confirmation_submission_maps_switch_outcomes() {
        assert_eq!(
            plan_confirmation_submission_to_outcome(&TransientSubmission::Selection(
                InlineListSelection::PlanApprovalSwitchBuild
            )),
            Some(PlanConfirmationOutcome::SwitchBuild)
        );
        assert_eq!(
            plan_confirmation_submission_to_outcome(&TransientSubmission::Selection(
                InlineListSelection::PlanApprovalSwitchAuto
            )),
            Some(PlanConfirmationOutcome::SwitchAuto)
        );
        assert_eq!(
            plan_confirmation_submission_to_outcome(&TransientSubmission::Selection(
                InlineListSelection::PlanApprovalExecute
            )),
            Some(PlanConfirmationOutcome::Execute)
        );
        assert_eq!(
            plan_confirmation_submission_to_outcome(&TransientSubmission::Selection(
                InlineListSelection::PlanApprovalAutoAccept
            )),
            Some(PlanConfirmationOutcome::AutoAccept)
        );
        assert_eq!(
            plan_confirmation_submission_to_outcome(&TransientSubmission::Selection(
                InlineListSelection::PlanApprovalEditPlan
            )),
            Some(PlanConfirmationOutcome::EditPlan)
        );
        // Unrecognized selection cancels.
        assert_eq!(
            plan_confirmation_submission_to_outcome(&TransientSubmission::Selection(
                InlineListSelection::ConfigAction("x".to_string())
            )),
            Some(PlanConfirmationOutcome::Cancel)
        );
        assert_eq!(
            plan_confirmation_submission_to_outcome(&TransientSubmission::Hotkey(TransientHotkeyAction::LaunchEditor)),
            Some(PlanConfirmationOutcome::EditPlan)
        );
    }

    #[test]
    fn plan_confirmation_request_includes_switch_items() {
        let req = build_plan_confirmation_request(&sample_plan(), false);
        let ListOverlayRequest { items, .. } = match req {
            TransientRequest::List(list) => list,
            _ => panic!("expected a list overlay request"),
        };
        let selections: Vec<InlineListSelection> = items.into_iter().filter_map(|item| item.selection).collect();
        assert!(
            selections
                .iter()
                .any(|s| matches!(s, InlineListSelection::PlanApprovalSwitchBuild))
        );
        assert!(
            selections
                .iter()
                .any(|s| matches!(s, InlineListSelection::PlanApprovalSwitchAuto))
        );
        assert!(selections.iter().any(|s| matches!(s, InlineListSelection::PlanApprovalExecute)));
    }

    #[tokio::test]
    async fn execute_plan_confirmation_emits_confirmation_overlay() {
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(command_tx);
        let mut session = InlineSession { handle: handle.clone(), events: event_rx };
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());

        event_tx
            .send(InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                InlineListSelection::PlanApprovalAutoAccept,
            ))))
            .expect("send confirmation selection");

        let outcome =
            execute_plan_confirmation(&handle, &mut session, sample_plan(), false, &ctrl_c_state, &ctrl_c_notify)
                .await
                .expect("confirmation overlay result");
        assert_eq!(outcome, PlanConfirmationOutcome::AutoAccept);

        let mut transcript_messages = Vec::new();
        let request = loop {
            let command = command_rx.try_recv().expect("confirmation command");
            match command {
                InlineCommand::AppendPastedMessage { kind: InlineMessageKind::Agent, text, .. } => {
                    transcript_messages.push(text);
                }
                InlineCommand::ShowTransient { request } => break request,
                _ => {}
            }
        };
        assert!(transcript_messages.iter().any(|text| text == "RAW fallback content"));

        match *request {
            TransientRequest::List(request) => {
                assert_eq!(request.title, "Ready to code?");
                assert!(request.lines.iter().any(|line| line.contains("A plan is ready")));
                assert!(
                    request
                        .items
                        .iter()
                        .any(|item| { item.selection == Some(InlineListSelection::PlanApprovalAutoAccept) })
                );
            }
            other => panic!("expected plan confirmation list, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn execute_plan_confirmation_edit_selection_returns_to_planning() {
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(command_tx);
        let mut session = InlineSession { handle: handle.clone(), events: event_rx };
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());

        event_tx
            .send(InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                InlineListSelection::PlanApprovalEditPlan,
            ))))
            .expect("send edit selection");

        let outcome =
            execute_plan_confirmation(&handle, &mut session, sample_plan(), false, &ctrl_c_state, &ctrl_c_notify)
                .await
                .expect("edit confirmation result");
        assert_eq!(outcome, PlanConfirmationOutcome::EditPlan);

        let request = loop {
            let command = command_rx.try_recv().expect("confirmation command");
            if let InlineCommand::ShowTransient { request } = command {
                break request;
            }
        };
        assert!(matches!(
            *request,
            TransientRequest::List(ListOverlayRequest { items, .. })
                if items.iter().any(|item| item.selection == Some(InlineListSelection::PlanApprovalEditPlan))
        ));
        loop {
            match command_rx.try_recv() {
                Ok(InlineCommand::CloseTransient) => break,
                Ok(_) => {}
                Err(error) => panic!("expected close command, got {error:?}"),
            }
        }
    }

    #[tokio::test]
    async fn execute_plan_confirmation_cancellation_is_not_approval() {
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(command_tx);
        let mut session = InlineSession { handle: handle.clone(), events: event_rx };
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());

        event_tx
            .send(InlineEvent::Transient(TransientEvent::Cancelled))
            .expect("send confirmation cancellation");

        let outcome =
            execute_plan_confirmation(&handle, &mut session, sample_plan(), false, &ctrl_c_state, &ctrl_c_notify)
                .await
                .expect("cancelled confirmation result");
        assert_eq!(outcome, PlanConfirmationOutcome::Cancel);

        loop {
            match command_rx.try_recv() {
                Ok(InlineCommand::CloseTransient) => break,
                Ok(_) => {}
                Err(error) => panic!("expected close command, got {error:?}"),
            }
        }
    }

    #[tokio::test]
    async fn execute_plan_confirmation_editor_hotkey_seeds_edit_command() {
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(command_tx);
        let mut session = InlineSession { handle: handle.clone(), events: event_rx };
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());

        event_tx
            .send(InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Hotkey(
                TransientHotkeyAction::LaunchEditor,
            ))))
            .expect("send editor hotkey");

        let outcome =
            execute_plan_confirmation(&handle, &mut session, sample_plan(), false, &ctrl_c_state, &ctrl_c_notify)
                .await
                .expect("editor confirmation result");
        assert_eq!(outcome, PlanConfirmationOutcome::EditPlan);

        let mut saw_input = false;
        while let Ok(command) = command_rx.try_recv() {
            match command {
                InlineCommand::SetInput(input) => {
                    assert_eq!(input, "/edit");
                    saw_input = true;
                }
                InlineCommand::CloseTransient => {}
                _ => {}
            }
        }
        assert!(saw_input, "editor hotkey should seed the /edit command");
    }
}
