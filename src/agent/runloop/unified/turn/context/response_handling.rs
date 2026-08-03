use super::*;
use crate::agent::runloop::unified::planning_workflow::{
    PlanArtifactError, ValidatedPlanArtifact, emit_plan_ready_events, persist_plan_draft, persisted_plan_is_ready,
    validate_plan_content,
};
use crate::agent::runloop::unified::ui_interaction_stream_helpers::render_compact_reasoning_block;

const DENIED_INTERVIEW_PLAN_SYNTHESIS_RETRY_DIRECTIVE: &str = "Planning recovery: the interactive interview is unavailable, and the previous response did not contain a completed plan. Do not ask another question or offer approval yet. Emit exactly one compact `<proposed_plan>` now from the repository evidence already in this conversation; include Summary, numbered `Action -> files/symbols -> verify:` steps, Validation, and short Assumptions. Do not emit tool calls.";

const PLAN_PSEUDO_TOOL_CALL_REPROMPT_DIRECTIVE: &str = "Planning: the previous response contained tool-call markup that was not executed — XML tool-call text is not a tool call. If you need more repository evidence, invoke tools through the tool-call channel now. Otherwise present the completed plan as one compact `<proposed_plan>` (Summary, numbered `Action -> files/symbols -> verify:` steps, Validation, short Assumptions). Do not emit XML tool-call markup as text.";
const INVALID_PLAN_REPAIR_DIRECTIVE: &str = "Planning recovery: the proposed plan is incomplete or still contains unresolved placeholders. Repair the persisted plan once using concrete repository evidence. Include Summary, numbered Implementation Steps, Test Cases and Validation, and Assumptions and Defaults. Resolve every open decision and do not ask for approval until the artifact is complete.";

/// Detect whether a planning-mode text response is a clarifying question
/// posed to the user rather than a plan or research prose. The deterministic
/// interview-denial recovery must NOT force plan synthesis when the model is
/// legitimately asking the user a question in plain text (the text-mode
/// equivalent of the unavailable `request_user_input` modal). Without this
/// check, the retry directive suppresses the question and the agent proceeds
/// to propose a plan without waiting for the user's answer (checkpoint
/// turn_856).
///
/// Heuristic: the last non-empty line ends with `?`. This is a strong signal
/// that the model is asking a question, and it does not match completed plans
/// (which end with Assumptions/Validation prose) or research dumps.
pub(super) fn looks_like_clarifying_question(text: &str) -> bool {
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .is_some_and(|last_line| last_line.trim().ends_with('?'))
}

impl<'a> TurnProcessingContext<'a> {
    fn reject_plan_artifact(
        &mut self,
        error: PlanArtifactError,
        allow_repair: bool,
    ) -> anyhow::Result<TurnHandlerOutcome> {
        use vtcode_core::utils::ansi::MessageStyle;

        let message = format!("Plan is not ready for approval: {error}");
        self.renderer.line(MessageStyle::Warning, &message)?;
        tracing::warn!(target: "vtcode.planning_workflow", error = %error, "plan artifact rejected before approval");
        if allow_repair && self.plan_session.plan_validation_repair_allowed() {
            self.plan_session.mark_plan_validation_repair_used();
            self.push_system_message(INVALID_PLAN_REPAIR_DIRECTIVE);
            return Ok(TurnHandlerOutcome::Continue);
        }
        self.renderer.line(
            MessageStyle::Warning,
            crate::agent::runloop::unified::planning_workflow_state::PLANNING_WORKFLOW_NO_APPROVAL_READY_PLAN_HINT,
        )?;
        Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: false }))
    }

    /// Schedule the one bounded plan-only retry allowed after a permanent
    /// interview denial. Keeping the transition here prevents callers from
    /// duplicating the denial/recovery state machine.
    pub(crate) fn retry_denied_interview_plan_synthesis(&mut self) -> bool {
        if !self.is_planning_active() || !self.plan_session.plan_synthesis_retry_allowed() {
            return false;
        }

        self.plan_session.mark_plan_synthesis_retry_used();
        self.push_system_message(DENIED_INTERVIEW_PLAN_SYNTHESIS_RETRY_DIRECTIVE);
        self.harness_state.retry_recovery_pass()
    }

    pub(crate) fn handle_assistant_response(
        &mut self,
        text: String,
        reasoning: Vec<ReasoningSegment>,
        reasoning_details: Option<Vec<String>>,
        response_streamed: bool,
        phase: Option<uni::AssistantPhase>,
    ) -> anyhow::Result<()> {
        let mut text = text;
        let detail_reasoning = reasoning_details
            .as_deref()
            .and_then(vtcode_core::llm::providers::common::extract_reasoning_text_from_serialized_details);
        if should_suppress_redundant_diff_recap(self.working_history, &text) {
            text.clear();
        }
        let has_visible_text = !text.trim().is_empty();
        let final_response_text = matches!(phase, Some(uni::AssistantPhase::FinalAnswer))
            .then(|| text.clone())
            .filter(|text| !text.trim().is_empty());
        if !reasoning.is_empty() || reasoning_details.as_ref().is_some_and(|details| !details.is_empty()) {
            tracing::info!(
                target: "vtcode.turn.metrics",
                metric = "reasoning_observed",
                run_id = %self.harness_state.run_id.0,
                turn_id = %self.harness_state.turn_id.0,
                phase = match phase {
                    Some(uni::AssistantPhase::Commentary) => "commentary",
                    Some(uni::AssistantPhase::FinalAnswer) => "final_answer",
                    None => "unspecified",
                },
                reasoning_segments = reasoning.len(),
                reasoning_details = reasoning_details.as_ref().map_or(0, Vec::len),
                has_detail_reasoning = detail_reasoning.is_some(),
                has_visible_text,
                response_streamed,
                "turn metric"
            );
        }

        if !response_streamed {
            use vtcode_core::utils::ansi::MessageStyle;

            if !text.trim().is_empty() {
                self.renderer.line(MessageStyle::Response, &text)?;
            }
            let mut rendered_reasoning = detail_reasoning.is_some().then(|| Vec::with_capacity(reasoning.len()));

            for segment in &reasoning {
                if let Some(stage) = &segment.stage {
                    self.handle.set_reasoning_stage(Some(stage.clone()));
                }

                let reasoning_text = &segment.text;
                if !reasoning_text.trim().is_empty() {
                    let duplicates_content = has_visible_text && reasoning_duplicates_content(reasoning_text, &text);
                    if !duplicates_content {
                        let compact = vtcode_commons::formatting::compact_reasoning_text(reasoning_text);
                        if compact.trim().is_empty() {
                            continue;
                        }
                        let rendered = render_compact_reasoning_block(self.renderer, reasoning_text)?;
                        if rendered && let Some(rendered_reasoning) = rendered_reasoning.as_mut() {
                            rendered_reasoning.push(compact);
                        }
                    }
                }
            }

            if let Some(detail_text) = detail_reasoning.as_deref() {
                let cleaned_detail = vtcode_commons::formatting::compact_reasoning_text(detail_text);
                let duplicates_content = has_visible_text && reasoning_duplicates_content(&cleaned_detail, &text);
                let duplicates_rendered = rendered_reasoning.as_ref().is_some_and(|rendered_reasoning| {
                    rendered_reasoning.iter().any(|existing: &String| {
                        reasoning_duplicates_content(existing, &cleaned_detail)
                            || reasoning_duplicates_content(&cleaned_detail, existing)
                    })
                });
                if !cleaned_detail.is_empty() && !duplicates_content && !duplicates_rendered {
                    render_compact_reasoning_block(self.renderer, detail_text)?;
                }
            }
            self.handle.set_reasoning_stage(None);
        }

        let combined_reasoning = build_combined_reasoning(&reasoning, detail_reasoning.as_deref());
        let include_reasoning = combined_reasoning
            .as_deref()
            .is_some_and(|combined_reasoning| !reasoning_duplicates_content(combined_reasoning, &text));
        let msg = uni::Message::assistant(text).with_phase(phase);
        let mut msg_with_reasoning = if include_reasoning {
            msg.with_reasoning(combined_reasoning)
        } else {
            msg
        };

        if let Some(details) = reasoning_details.filter(|d| !d.is_empty()) {
            let payload = details
                .into_iter()
                .map(|detail| parse_reasoning_detail_value(&detail))
                .collect::<Vec<_>>();
            msg_with_reasoning = msg_with_reasoning.with_reasoning_details(Some(payload));
        }

        if !msg_with_reasoning.content.as_text().is_empty()
            || msg_with_reasoning.reasoning.is_some()
            || msg_with_reasoning.reasoning_details.is_some()
        {
            push_assistant_message(self.working_history, msg_with_reasoning);
        }

        if let Some(final_response_text) = final_response_text {
            self.harness_state.mark_final_response_rendered();
            if self.harness_emitter.is_none() || self.harness_state.streamed_response_event_emitted() {
                self.harness_state.mark_final_response_event_emitted();
            } else if !self.harness_state.final_response_event_emitted()
                && let Some(emitter) = self.harness_emitter
            {
                match emitter.emit_assistant_message(&self.harness_state.turn_id.0, &final_response_text) {
                    Ok(()) => self.harness_state.mark_final_response_event_emitted(),
                    Err(err) => tracing::warn!(error = %err, "final assistant message harness emission failed"),
                }
            }
        }

        Ok(())
    }

    pub(crate) async fn handle_text_response(
        &mut self,
        text: String,
        reasoning: Vec<ReasoningSegment>,
        reasoning_details: Option<Vec<String>>,
        proposed_plan: Option<String>,
        response_streamed: bool,
    ) -> anyhow::Result<TurnHandlerOutcome> {
        let recovery_pass_response = self.is_recovery_active() && self.recovery_pass_used();
        let tool_free_recovery_pass = recovery_pass_response && self.recovery_is_tool_free();
        // Tool-free recovery is terminal: the model's text IS the final answer.
        // Some providers (e.g. MiniMax) emit a noise prefix like `]<]minimax[>[`
        // before/instead of real content. When the model has nothing to
        // synthesize, this residue becomes the user-visible final answer — the
        // "agent just stops with garbage" symptom (checkpoints turn_609/613).
        // Strip known noise and, if nothing meaningful remains, substitute a
        // clear fallback so the user gets an actionable message instead of
        // provider noise.
        // Strip provider noise (e.g. MiniMax `]<]minimax[>[`) from ALL assistant
        // text — commentary, normal final answers, and recovery final answers.
        // This prevents noise from leaking into the user-visible output and,
        // more importantly, from being echoed back to the API via
        // `working_history` on follow-up calls (polluted context degrades
        // subsequent responses and contributes to post-tool follow-up
        // failures). For tool-free recovery passes, additionally substitute a
        // fallback when nothing meaningful remains after stripping.
        let text = if tool_free_recovery_pass {
            crate::agent::runloop::unified::turn::provider_noise::sanitize_recovery_answer(text)
        } else {
            crate::agent::runloop::unified::turn::provider_noise::strip_provider_noise(&text)
        };
        // Plan-mode salvage: a model with no tool schemas on the wire (or a
        // confused checkpoint) sometimes answers with XML-ish tool-call markup
        // as text. No textual parser could execute it, and in plan mode any
        // text ends the turn, so the raw markup became the user-visible final
        // answer and leaked into history, ATIF, and harness logs
        // (turn_887/turn_888). Strip the markup from the stored/visible text;
        // a bounded re-prompt below gives the model a chance to call tools
        // natively or present the plan instead.
        let pseudo_tool_call_markup_detected = self.is_planning_active()
            && !tool_free_recovery_pass
            && proposed_plan.is_none()
            && crate::agent::runloop::text_tools::contains_pseudo_tool_call_markers(&text);
        let text = if pseudo_tool_call_markup_detected {
            crate::agent::runloop::text_tools::strip_textual_tool_call_regions(&text)
                .trim()
                .to_string()
        } else {
            text
        };
        let denied_interview_plan_retry = self.is_planning_active()
            && !tool_free_recovery_pass
            && proposed_plan.is_none()
            && !text.trim().is_empty()
            && self.plan_session.plan_synthesis_retry_allowed();
        let denied_interview_recovery_retry = self.is_planning_active()
            && tool_free_recovery_pass
            && proposed_plan.is_none()
            && self.plan_session.plan_synthesis_retry_allowed();
        let denied_interview_without_ready_plan = self.is_planning_active()
            && self.plan_session.is_interview_denied()
            && proposed_plan.is_none()
            && !persisted_plan_is_ready(&self.tool_registry.planning_workflow_state()).await
            && !looks_like_clarifying_question(&text);
        let text = if denied_interview_without_ready_plan {
            crate::agent::runloop::unified::planning_workflow_state::PLANNING_WORKFLOW_NO_APPROVAL_READY_PLAN_HINT
                .to_string()
        } else {
            text
        };
        let final_text = text.clone();
        let consecutive_relaxed = self.harness_state.consecutive_relaxed_continuations;
        let continuation_decision = if tool_free_recovery_pass {
            // Tool-free recovery is terminal: the text produced during recovery
            // IS the final answer. Allowing continuation here would call
            // `finish_recovery_pass()` (deactivating recovery), re-enable tools
            // on the next iteration, and — if the follow-up fails again —
            // re-trigger recovery, producing an infinite cycle that no existing
            // bound catches (`consecutive_relaxed_continuations` is bypassed by
            // non-relaxed "recent_tool_activity" continuations that reset the
            // counter to 0, and `MAX_RECOVERY_RETRIES` only counts retries
            // within a single pass). Evaluate continuation intent solely to
            // populate diagnostic fields for the tracing log; the decision is
            // always to end the turn.
            let decision = evaluate_interim_text_continuation(
                self.full_auto,
                self.is_planning_active(),
                self.working_history,
                &text,
                consecutive_relaxed,
            );
            InterimTextContinuationDecision {
                should_continue: false,
                reason: "tool_free_recovery_terminal",
                is_interim_progress: decision.is_interim_progress,
                last_user_follow_up: decision.last_user_follow_up,
                recent_tool_activity: decision.recent_tool_activity,
                last_user_requested_progressive_work: decision.last_user_requested_progressive_work,
                is_relaxed_continuation: false,
            }
        } else {
            evaluate_interim_text_continuation(
                self.full_auto,
                self.is_planning_active(),
                self.working_history,
                &text,
                consecutive_relaxed,
            )
        };

        // Track consecutive relaxed continuations to prevent infinite loops.
        if continuation_decision.should_continue && continuation_decision.is_relaxed_continuation {
            self.harness_state.consecutive_relaxed_continuations += 1;
        } else if continuation_decision.should_continue {
            // Non-relaxed continuation resets the counter
            self.harness_state.consecutive_relaxed_continuations = 0;
        } else {
            // Turn is ending, reset the counter
            self.harness_state.consecutive_relaxed_continuations = 0;
        }

        let assistant_phase = if continuation_decision.should_continue {
            Some(uni::AssistantPhase::Commentary)
        } else {
            Some(uni::AssistantPhase::FinalAnswer)
        };
        self.handle_assistant_response(text, reasoning, reasoning_details, response_streamed, assistant_phase)?;

        // Count this text response so the recovery loop can short-circuit
        // when the model has already produced a final answer but the loop
        // keeps re-prompting. See `MAX_ASSISTANT_TEXT_RESPONSES_PER_TURN`.
        self.harness_state.record_assistant_text_response();

        if recovery_pass_response {
            self.finish_recovery_pass();
        }

        // A tool-free pass is normally terminal, but a permanently denied
        // interview has one additional bounded contract: it must produce a
        // real draft before the user can approve anything. If the provider
        // ignored the recovery directive and returned prose without a plan,
        // retry once while tools remain disabled instead of ending mid-turn
        // with no approval-ready draft.
        //
        // EXCEPTION: if the text is a clarifying question (the text-mode
        // equivalent of the unavailable interview modal), end the turn so the
        // user can answer it. Forcing plan synthesis here would suppress the
        // question and proceed to propose a plan without user input
        // (checkpoint turn_856).
        if denied_interview_recovery_retry {
            if looks_like_clarifying_question(&final_text) {
                tracing::info!(
                    target: "vtcode.planning_workflow",
                    "denied interview recovery produced a clarifying question; ending turn for user input instead of retrying plan synthesis"
                );
                // Fall through to normal turn completion — the question is
                // already in working_history as the assistant's final answer.
            } else if self.retry_denied_interview_plan_synthesis() {
                tracing::info!(
                    target: "vtcode.planning_workflow",
                    "retrying tool-free synthesis after denied interview returned no plan"
                );
                return Ok(TurnHandlerOutcome::Continue);
            }
        }

        // A permanent interview denial is different from a cancelled
        // interview: the model must still produce a real draft before the
        // user can approve it. The denial diagnostic is advisory, so some
        // models answer only with "type yes" instead of emitting a plan.
        // Give that response one bounded synthesis retry. This keeps the
        // approval path draft-backed without re-enabling the unavailable
        // interview tool or allowing an unbounded continuation loop.
        //
        // EXCEPTION: a clarifying question is the text-mode equivalent of
        // the unavailable interview modal — end the turn for user input
        // instead of suppressing it with a forced synthesis retry.
        if denied_interview_plan_retry && !looks_like_clarifying_question(&final_text) {
            self.plan_session.mark_plan_synthesis_retry_used();
            self.push_system_message(DENIED_INTERVIEW_PLAN_SYNTHESIS_RETRY_DIRECTIVE);
            tracing::info!(
                target: "vtcode.planning_workflow",
                "retrying denied interview response as a bounded plan synthesis"
            );
            return Ok(TurnHandlerOutcome::Continue);
        }

        // Plan-mode pseudo-tool-call reprompt: a model with no tool schemas on
        // the wire (or a confused checkpoint) sometimes emits XML-ish
        // tool-call markup as text. No textual parser could execute it, and in
        // plan mode any text ends the turn, so the raw markup previously
        // became the user-visible final answer and leaked into history, ATIF,
        // and harness logs (turn_887/turn_888). The markup was already stripped
        // from the stored text above; give the model a bounded chance to call
        // tools natively or present the plan instead of ending mid-turn with
        // cleaned-up prose. When the reprompt budget is exhausted, fall through
        // to normal turn completion — the already-stripped text guarantees raw
        // markup never reaches the user.
        if pseudo_tool_call_markup_detected && self.plan_session.plan_pseudo_tool_call_reprompt_allowed() {
            self.plan_session.mark_plan_pseudo_tool_call_reprompt_used();
            self.push_system_message(PLAN_PSEUDO_TOOL_CALL_REPROMPT_DIRECTIVE);
            tracing::info!(
                target: "vtcode.planning_workflow",
                "re-prompting after pseudo-tool-call markup in plan mode"
            );
            return Ok(TurnHandlerOutcome::Continue);
        }

        tracing::info!(
            target: "vtcode.turn.metrics",
            metric = "text_response_decision",
            run_id = %self.harness_state.run_id.0,
            turn_id = %self.harness_state.turn_id.0,
            should_continue = continuation_decision.should_continue,
            reason = continuation_decision.reason,
            is_interim_progress = continuation_decision.is_interim_progress,
            last_user_follow_up = continuation_decision.last_user_follow_up,
            recent_tool_activity = continuation_decision.recent_tool_activity,
            last_user_requested_progressive_work =
                continuation_decision.last_user_requested_progressive_work,
            recovery_pass_response,
            tool_free_recovery_pass,
            planning_workflow = self.is_planning_active(),
            full_auto = self.full_auto,
            history_len = self.working_history.len(),
            "turn metric"
        );

        if continuation_decision.should_continue {
            push_system_directive_once(self.working_history, AUTONOMOUS_CONTINUE_DIRECTIVE);
            return Ok(TurnHandlerOutcome::Continue);
        }

        if let Some(hooks) = self.lifecycle_hooks {
            let outcome = hooks.run_stop(&final_text, self.harness_state.stop_hook_active).await?;
            crate::agent::runloop::unified::turn::utils::render_hook_messages(self.renderer, &outcome.messages)?;
            if let Some(reason) = outcome.block_reason {
                push_system_directive_once(self.working_history, &reason);
                self.harness_state.stop_hook_active = true;
                return Ok(TurnHandlerOutcome::Continue);
            }
        }
        self.harness_state.stop_hook_active = false;

        if let Some(plan_text) = proposed_plan {
            let planning_active = self.is_planning_active();
            tracing::info!(
                target: "vtcode.planning_workflow",
                plan_ready = true,
                planning_active,
                "completed plan reached approval handoff"
            );
            // Persist before publishing the approval request so consumers that
            // follow the event's plan_file can read the completed draft.
            let validation = validate_plan_content(&plan_text);
            if !validation.is_ready() {
                let error = PlanArtifactError::Invalid { reasons: validation.reasons().join("; ") };
                return self.reject_plan_artifact(error, !tool_free_recovery_pass);
            }

            let persisted = match persist_plan_draft(&self.tool_registry.planning_workflow_state(), &plan_text).await {
                Ok(persisted) => persisted,
                Err(error) => {
                    let error = PlanArtifactError::Persistence { reason: error.to_string() };
                    return self.reject_plan_artifact(error, false);
                }
            };
            if !persisted.validation.is_ready() {
                let error = PlanArtifactError::Invalid { reasons: persisted.validation.reasons().join("; ") };
                return self.reject_plan_artifact(error, !tool_free_recovery_pass);
            }
            if !persisted_plan_is_ready(&self.tool_registry.planning_workflow_state()).await {
                let error = PlanArtifactError::Persistence {
                    reason: "plan, sidecar tracker, and workspace tracker were not published completely".to_string(),
                };
                return self.reject_plan_artifact(error, false);
            }
            let plan = match ValidatedPlanArtifact::from_text(persisted.plan_file.clone(), plan_text.clone()) {
                Ok(plan) => plan,
                Err(error) => return self.reject_plan_artifact(error, !tool_free_recovery_pass),
            };
            let plan_state = self.tool_registry.planning_workflow_state();
            emit_plan_ready_events(
                self.plan_session,
                &plan_state,
                self.harness_emitter,
                &self.harness_state.run_id.0,
                &self.harness_state.turn_id.0,
                &plan_text,
            )
            .await;

            let require_confirmation = self.vt_cfg.map(|cfg| cfg.agent.require_plan_confirmation).unwrap_or(true);
            let supports_inline = self.renderer.supports_inline_ui();
            tracing::info!(
                target: "vtcode.planning_workflow",
                plan_ready = true,
                require_confirmation,
                supports_inline_ui = supports_inline,
                "plan approval overlay condition check"
            );
            let approval_route = crate::agent::runloop::unified::planning_workflow::plan_approval_route(
                require_confirmation,
                supports_inline,
                self.skip_confirmations,
                self.full_auto,
            );
            tracing::info!(
                target: "vtcode.planning_workflow",
                ?approval_route,
                "plan approval route selected"
            );
            if approval_route == crate::agent::runloop::unified::planning_workflow::PlanApprovalRoute::Inline {
                use crate::agent::runloop::unified::planning_workflow::{
                    PlanApprovalRequestContext, PlanApprovalTelemetryContext, execute_plan_approval,
                };
                return execute_plan_approval(
                    self.tool_registry,
                    self.plan_session,
                    self.handle,
                    self.session,
                    self.ctrl_c_state,
                    self.ctrl_c_notify,
                    PlanApprovalRequestContext {
                        plan: &plan,
                        active_agent_name: self.active_primary_agent.active().name(),
                        skip_confirmations: self.skip_confirmations,
                        context_usage_percent: self.context_manager.context_usage_percent(
                            self.vt_cfg
                                .map(|cfg| cfg.context.max_context_tokens)
                                .unwrap_or_else(vtcode_config::context::default_max_context_tokens),
                        ),
                    },
                    PlanApprovalTelemetryContext {
                        emitter: self.harness_emitter,
                        thread_id: &self.harness_state.run_id.0,
                        turn_id: &self.harness_state.turn_id.0,
                    },
                )
                .await;
            }

            use vtcode_core::utils::ansi::MessageStyle;
            self.renderer.line(MessageStyle::Info, "Plan ready for approval:")?;
            self.renderer.line(MessageStyle::Response, &plan_text)?;
            if approval_route == crate::agent::runloop::unified::planning_workflow::PlanApprovalRoute::Headless {
                self.renderer.line(
                    MessageStyle::Info,
                    "Plan is awaiting approval. Type `approve`, `implement`, or `yes` to begin execution, or `edit` to revise the plan.",
                )?;
                return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed {
                    plan_approved_execution_pending: false,
                }));
            }

            self.renderer
                .line(MessageStyle::Info, "Plan approved by the active execution policy; starting implementation.")?;
            let handoff = crate::agent::runloop::unified::planning_workflow::complete_approved_plan_handoff(
                self.tool_registry,
                self.plan_session,
                self.handle,
                plan,
                self.active_primary_agent.active().name(),
                true,
                crate::agent::runloop::unified::planning_workflow::PlanExecutionContext::Current,
            )
            .await;
            let handoff = match handoff {
                Ok(handoff) => handoff,
                Err(error) => {
                    tracing::warn!(target: "vtcode.planning_workflow", error = %error, "automatic approved-plan handoff blocked");
                    crate::agent::runloop::unified::planning_workflow::resolve_plan_approval(
                        self.plan_session,
                        self.harness_emitter,
                        &self.harness_state.run_id.0,
                        &self.harness_state.turn_id.0,
                        vtcode_core::exec::events::PlanApprovalDecision::Cancel,
                        true,
                    );
                    let message = format!("Plan execution is blocked: {error}");
                    self.renderer.line(MessageStyle::Error, &message)?;
                    return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed {
                        plan_approved_execution_pending: false,
                    }));
                }
            };
            crate::agent::runloop::unified::planning_workflow::resolve_plan_approval(
                self.plan_session,
                self.harness_emitter,
                &self.harness_state.run_id.0,
                &self.harness_state.turn_id.0,
                vtcode_core::exec::events::PlanApprovalDecision::AutoAccept,
                true,
            );
            let execution_agent = handoff.execution_agent;
            let handoff_skip_confirmations = handoff.skip_confirmations;
            if let Some(agent) = execution_agent {
                return Ok(TurnHandlerOutcome::SwitchPrimaryAgentWithPolicy {
                    agent,
                    skip_confirmations: handoff_skip_confirmations,
                    execution_context: crate::agent::runloop::unified::planning_workflow::PlanExecutionContext::Current,
                });
            }
            return Ok(TurnHandlerOutcome::BreakWithPolicy {
                result: TurnLoopResult::Completed { plan_approved_execution_pending: true },
                skip_confirmations: handoff_skip_confirmations,
                execution_context: crate::agent::runloop::unified::planning_workflow::PlanExecutionContext::Current,
            });
        }

        Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: false }))
    }
}

// NOTE: Provider-noise stripping (MiniMax `]<]minimax[>[` and similar) has been
// centralized in `turn::provider_noise`. All call sites — textual tool parsers,
// response handling, and the live stream renderer — delegate to
// `strip_provider_noise` / `sanitize_recovery_answer` there. See that module
// for the canonical noise vocabulary and comprehensive tests.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;

    /// Unparseable pseudo-tool-call markup: the `<tools:call>` name is empty,
    /// so every textual parser rejects it, but the pseudo-marker scan still
    /// sees `<tool_call`. Mirrors the raw-XML leak from turn_887/turn_888.
    const BROKEN_MARKUP_RESPONSE: &str =
        "I need to inspect the workspace.\n<tool_call>\n<tools:call name=\"\">\n</tools:call>\n</tool_call>";

    #[tokio::test]
    async fn plan_mode_pseudo_tool_call_markup_is_stripped_and_reprompts_once() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.enable_planning();
        let mut ctx = backing.turn_processing_context();

        let outcome = ctx
            .handle_text_response(BROKEN_MARKUP_RESPONSE.to_string(), Vec::new(), None, None, false)
            .await
            .expect("text response should be handled");

        assert!(
            matches!(outcome, TurnHandlerOutcome::Continue),
            "plan-mode pseudo-tool-call markup should re-prompt instead of ending the turn"
        );

        let assistant_texts: Vec<String> = ctx
            .working_history
            .iter()
            .filter(|message| message.role == uni::MessageRole::Assistant)
            .map(|message| message.content.as_text().into_owned())
            .collect();
        assert!(
            assistant_texts
                .iter()
                .any(|text| text.contains("I need to inspect the workspace.")),
            "the prose part of the response should be preserved: {assistant_texts:?}"
        );
        assert!(
            assistant_texts.iter().all(|text| !text.contains("<tool_call")),
            "raw tool-call markup must never be stored in history: {assistant_texts:?}"
        );

        let directive_present = ctx.working_history.iter().any(|message| {
            message.role == uni::MessageRole::System && message.content.as_text().contains("not executed")
        });
        assert!(directive_present, "a re-prompt directive should be pushed into history");
    }

    #[tokio::test]
    async fn plan_mode_pseudo_tool_call_reprompt_is_bounded() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.enable_planning();
        let mut ctx = backing.turn_processing_context();
        for _ in 0..crate::agent::runloop::unified::planning_workflow_state::MAX_PLAN_PSEUDO_TOOL_CALL_REPROMPTS {
            ctx.plan_session.mark_plan_pseudo_tool_call_reprompt_used();
        }

        let outcome = ctx
            .handle_text_response(BROKEN_MARKUP_RESPONSE.to_string(), Vec::new(), None, None, false)
            .await
            .expect("text response should be handled");

        assert!(
            matches!(outcome, TurnHandlerOutcome::Break(_)),
            "an exhausted reprompt budget must end the turn instead of looping"
        );
        let assistant_texts: Vec<String> = ctx
            .working_history
            .iter()
            .filter(|message| message.role == uni::MessageRole::Assistant)
            .map(|message| message.content.as_text().into_owned())
            .collect();
        assert!(
            assistant_texts.iter().all(|text| !text.contains("<tool_call")),
            "even with the budget exhausted, raw markup must be stripped from the final answer: {assistant_texts:?}"
        );
    }

    #[tokio::test]
    async fn build_mode_pseudo_tool_call_markup_keeps_existing_behavior() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();

        let outcome = ctx
            .handle_text_response(BROKEN_MARKUP_RESPONSE.to_string(), Vec::new(), None, None, false)
            .await
            .expect("text response should be handled");

        assert!(
            matches!(outcome, TurnHandlerOutcome::Break(_)),
            "outside planning, text responses still end the turn (no new reprompt path)"
        );
    }
}
