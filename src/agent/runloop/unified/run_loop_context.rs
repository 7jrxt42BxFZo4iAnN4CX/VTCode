use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hashbrown::{HashMap, HashSet};
use tokio::sync::RwLock;
use vtcode_config::core::permissions::AgentPermissionsConfig;
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::config::PermissionsConfig;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::{ApprovalRecorder, ToolRegistry, ToolResultCache};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_ui::tui::app::{InlineHandle, InlineSession};

use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnRunId(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnPhase {
    Preparing,
    Requesting,
    ExecutingTools,
    Finalizing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnExecutionPhase {
    Preparing,
    Requesting,
    ExecutingTools,
    Finalizing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryPhase {
    Inactive,
    Pending,
    InPass,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryMode {
    ToolEnabledRetry,
    ToolFreeSynthesis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToolBudgetWarning {
    pub used: usize,
    pub max: usize,
    pub remaining: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToolBudgetExhaustion {
    pub used: usize,
    pub max: usize,
    pub remaining: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToolBudgetExhaustionNotice {
    pub exhaustion: ToolBudgetExhaustion,
    pub first_notice: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToolWallClockExhaustion {
    pub max_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToolWallClockExhaustionNotice {
    pub exhaustion: ToolWallClockExhaustion,
    pub first_notice: bool,
}

pub(crate) const TOOL_BUDGET_WARNING_THRESHOLD: f64 = 0.75;

impl ToolBudgetWarning {
    pub(crate) fn system_message(self) -> String {
        format!(
            "Tool-call budget warning: {}/{} used; {} remaining for this turn. Use targeted extraction/batching before additional tool calls.",
            self.used, self.max, self.remaining
        )
    }

    pub(crate) fn log_threshold_reached(self, path: &'static str) {
        tracing::info!(used = self.used, max = self.max, remaining = self.remaining, "{path}");
    }
}

impl ToolBudgetExhaustion {
    pub(crate) fn policy_violation_message(self) -> String {
        format!("Policy violation: exceeded max tool calls per turn ({})", self.max)
    }

    /// Compact stub returned for the 2nd+ rejected calls in the same batch so
    /// the full policy message isn't repeated N times and context stays clean.
    pub(crate) fn skipped_call_message(self) -> String {
        "Tool-call budget exhausted for this turn; call skipped.".to_string()
    }

    /// System directive pushed once (after all tool responses in the batch)
    /// telling the model that tools are disabled for the rest of the turn and
    /// it must synthesize a final answer from already-gathered outputs.
    /// Mirrors `ToolWallClockExhaustion::synthesis_directive_message`.
    pub(crate) fn synthesis_directive_message(self) -> String {
        debug_assert!(self.max > 0, "disabled tool-call caps must not emit exhaustion");
        format!(
            "Tool-call budget exhausted for this turn ({}/{}). Tools are disabled for the rest of this turn. Do NOT emit more tool calls. Synthesize your final answer now from the tool outputs already gathered in this conversation.",
            self.used, self.max
        )
    }
}

impl ToolWallClockExhaustion {
    pub(crate) fn policy_violation_message(self) -> String {
        format!("Policy violation: exceeded tool wall clock budget ({}s)", self.max_secs)
    }

    /// Compact stub returned for the 2nd+ rejected calls in the same batch so
    /// the full policy message isn't repeated N times and context stays clean.
    pub(crate) fn skipped_call_message(self) -> String {
        "Tool wall-clock budget exhausted for this turn; call skipped.".to_string()
    }

    /// System directive pushed once (after all tool responses in the batch)
    /// telling the model that tools are disabled for the rest of the turn and
    /// it must synthesize a final answer from already-gathered outputs. This is
    /// the in-turn synthesis nudge that the raw per-call policy errors lack.
    pub(crate) fn synthesis_directive_message(self) -> String {
        format!(
            "Tool wall-clock budget exhausted for this turn ({}s). Tools are disabled for the rest of this turn. Do NOT emit more tool calls. Synthesize your final answer now from the tool outputs already gathered in this conversation.",
            self.max_secs
        )
    }
}

impl From<TurnPhase> for TurnExecutionPhase {
    fn from(value: TurnPhase) -> Self {
        match value {
            TurnPhase::Preparing => Self::Preparing,
            TurnPhase::Requesting => Self::Requesting,
            TurnPhase::ExecutingTools => Self::ExecutingTools,
            TurnPhase::Finalizing => Self::Finalizing,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnExecutionSnapshot {
    pub run_id: String,
    pub turn_id: String,
    pub phase: TurnExecutionPhase,
    pub max_tool_calls: usize,
    pub max_tool_wall_clock_secs: u64,
    pub max_tool_retries: u32,
}

/// Tracks action patterns across turn boundaries to detect loops that span
/// multiple turns.  Constructed once per session and carried forward across
/// turns, unlike `HarnessTurnState` which is fresh each turn.
///
/// Each turn produces a "fingerprint" — a hash of the sorted set of tool
/// signatures used in that turn.  If the same fingerprint appears 2+ times
/// in the sliding window, a cross-turn loop is detected.
///
/// Also tracks consecutive turns with zero mutating tool calls to detect
/// "stuck" states where the agent only reads without making progress.
pub(crate) struct CrossTurnTracker {
    /// Rolling window of per-turn action fingerprints.
    turn_fingerprints: VecDeque<u64>,
    /// Maximum window size for cross-turn loop detection.
    window_size: usize,
    /// Consecutive turns with zero mutating tool calls.
    zero_mutation_turns: usize,
}

/// Number of consecutive zero-mutation turns before a HARD STOP fires.
const STUCK_ZERO_MUTATION_THRESHOLD: usize = 3;

impl CrossTurnTracker {
    pub(crate) fn new() -> Self {
        Self {
            turn_fingerprints: VecDeque::with_capacity(8),
            window_size: 8,
            zero_mutation_turns: 0,
        }
    }

    /// Seal the current turn: compute a fingerprint from the provided tool
    /// signatures, check for cross-turn loops and stuck states.
    ///
    /// - `read_only_signatures`: signatures of read-only tool calls this turn.
    /// - `written_files`: paths of files written this turn.
    /// - `shell_command`: last shell command signature, if any.
    /// - `planning_active`: whether the planning workflow is currently active.
    ///
    /// Returns a warning string if a loop or stuck pattern is detected.
    pub(crate) fn seal_turn(
        &mut self,
        read_only_signatures: &[String],
        written_files: &HashSet<String>,
        shell_command: Option<&str>,
        planning_active: bool,
    ) -> Option<String> {
        let mut signatures: Vec<String> = read_only_signatures.to_vec();
        for path in written_files {
            signatures.push(format!("write::{path}"));
        }
        if let Some(cmd) = shell_command {
            signatures.push(cmd.to_string());
        }

        let had_mutation = !written_files.is_empty();

        // Compute fingerprint from sorted signatures so order doesn't matter.
        let fingerprint = if signatures.is_empty() {
            0
        } else {
            let mut sorted: Vec<&str> = signatures.iter().map(String::as_str).collect();
            sorted.sort_unstable();
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            for sig in &sorted {
                sig.hash(&mut hasher);
            }
            hasher.finish()
        };

        // Check cross-turn loop before pushing this turn's fingerprint.
        let loop_warning = if fingerprint != 0 && self.turn_fingerprints.contains(&fingerprint) {
            Some(
                "Cross-turn loop detected: the same set of tool actions has repeated across \
                 consecutive turns. Break the pattern by trying a different approach or \
                 synthesizing a final answer from existing context."
                    .to_string(),
            )
        } else {
            None
        };

        if fingerprint != 0 {
            if self.turn_fingerprints.len() >= self.window_size {
                self.turn_fingerprints.pop_front();
            }
            self.turn_fingerprints.push_back(fingerprint);
        }

        // Track zero-mutation turns for stuck detection.
        if !signatures.is_empty() {
            if had_mutation {
                self.zero_mutation_turns = 0;
            } else if !planning_active {
                self.zero_mutation_turns = self.zero_mutation_turns.saturating_add(1);
            }
        }

        // Return loop warning first (higher priority), then stuck warning.
        if loop_warning.is_some() {
            return loop_warning;
        }

        if !planning_active && self.zero_mutation_turns >= STUCK_ZERO_MUTATION_THRESHOLD {
            return Some(format!(
                "No progress detected for {} consecutive turns (all read-only tool calls, \
                 no file mutations or command executions). Synthesize a final answer from \
                 existing context or ask the user for guidance.",
                self.zero_mutation_turns
            ));
        }

        None
    }

    /// Check if the tracker has detected a stuck pattern (for diagnostics).
    #[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
    pub(crate) fn zero_mutation_turns(&self) -> usize {
        self.zero_mutation_turns
    }
}

pub(crate) struct HarnessTurnState {
    pub run_id: TurnRunId,
    pub turn_id: TurnId,
    pub phase: TurnPhase,
    pub turn_started_at: Instant,
    /// Time spent in explicit command waits is excluded from the ordinary
    /// per-turn harness wall-clock budget.
    wait_started_at: Option<Instant>,
    excluded_wait_duration: Duration,
    pub tool_calls: usize,
    requested_tool_calls: u32,
    admitted_tool_calls: u32,
    failed_tool_calls: u32,
    denied_tool_calls: u32,
    preflight_failures: u32,
    reused_results: u32,
    spooled_results: u32,
    raw_spooled_bytes: u64,
    model_visible_output_bytes: u64,
    recovery_activations: u32,
    pub blocked_tool_calls: usize,
    pub consecutive_blocked_tool_calls: usize,
    /// Counts consecutive malformed/schema-invalid tool calls independently
    /// from policy denials. A valid admitted call resets this streak.
    pub consecutive_preflight_failures: usize,
    /// Counts how many times the agent emitted an assistant *text* response
    /// (no tool calls) in this turn.  Used to short-circuit the recovery
    /// loop when the model has already produced a final answer but recovery
    /// iteration keeps re-prompting it.  Reset every turn.
    pub assistant_text_responses_in_turn: u32,
    /// Whether a non-empty final assistant response was rendered for this turn.
    /// This is separate from conversation history because recovery code can
    /// append a message without sending it through the user-facing renderer.
    final_response_rendered: bool,
    /// Whether the final assistant response reached the harness event stream.
    /// The stream is optional in interactive-only runs, where the state is
    /// treated as emitted once the response was rendered.
    final_response_event_emitted: bool,
    /// Whether the streaming bridge emitted assistant output for this turn.
    /// This is kept separate from the final-response flag because a streamed
    /// commentary preamble can precede tool calls and is not itself a final
    /// answer.
    streamed_response_event_emitted: bool,
    /// Whether the final response was produced by deterministic recovery
    /// fallback rather than by a successful model synthesis.
    final_response_was_fallback: bool,
    pub consecutive_spool_chunk_reads: usize,
    pub consecutive_same_shell_command_runs: usize,
    pub last_shell_command_signature: Option<String>,
    pub consecutive_same_file_read_family_calls: usize,
    last_file_read_family_signature: Option<String>,
    /// Per-file-path read count, independent of slice (offset/limit/raw).
    /// Catches paginated reads of the same file that the slice-aware family
    /// key lets through. Reset every turn.
    file_read_path_counts: HashMap<String, usize>,
    pub(crate) seen_successful_readonly_signatures: HashSet<String>,
    streamed_tool_call_item_ids: HashMap<String, String>,
    pub stop_hook_active: bool,
    pub seen_task_tracker_create_signatures: HashSet<String>,
    pub replaceable_task_tracker_block: Option<Vec<String>>,
    pub recently_written_files: HashSet<String>,
    pub tool_budget_warning_emitted: bool,
    pub tool_budget_exhausted_emitted: bool,
    /// Whether the first-notice wall-clock-exhaustion policy message has been
    /// emitted this turn. Mirrors `tool_budget_exhausted_emitted` so the full
    /// policy-violation message is sent once and subsequent rejected calls in
    /// the same batch get a compact stub instead of repeating it.
    pub wall_clock_exhausted_emitted: bool,
    /// Set when the first wall-clock rejection fires; consumed after the tool
    /// batch by the handler to push a single "synthesize now" system directive
    /// *after* all tool responses (never interleaved between them).
    pub wall_clock_directive_pending: bool,
    /// Set when the first tool-call-budget rejection fires; consumed after the
    /// tool batch to push a single "synthesize now" system directive, mirroring
    /// `wall_clock_directive_pending`. Without this, tool-call budget
    /// exhaustion hard-broke the turn as `Blocked` with no synthesis pass, so
    /// plan mode never produced a plan (checkpoint turn_647 follow-up).
    pub tool_budget_directive_pending: bool,
    pub recovery_reason: Option<String>,
    recovery_phase: RecoveryPhase,
    recovery_mode: Option<RecoveryMode>,
    recovery_retry_count: u8,
    /// Set for the one post-tool provider-recovery pass that must compact the
    /// older prefix before the next tool-enabled request.
    post_tool_compaction_pending: bool,
    /// Set when the provider identified the failed follow-up as a context
    /// capacity error. A failed/no-op recovery compaction must then produce a
    /// blocked handoff instead of retrying the same oversized request.
    post_tool_context_capacity_failure: bool,
    /// Set when the required recovery compaction failed or had no reducible
    /// prefix after a context-capacity rejection.
    post_tool_context_compaction_failed: bool,
    /// Explicit one-shot guard for the tool-enabled retry. This is separate
    /// from the tool-free recovery cycle counter because the two modes have
    /// different budgets and completion semantics.
    post_tool_tool_enabled_retry_used: bool,
    /// Counts how many times the post-tool follow-up failure path has
    /// scheduled a tool-free recovery pass within a single turn. Bounded by
    /// `MAX_POST_TOOL_RECOVERY_CYCLES` in the turn loop as a defense-in-depth
    /// backstop against any regression that re-triggers recovery cyclically.
    /// Resets naturally per turn because each turn constructs a fresh
    /// `HarnessTurnState`.
    post_tool_recovery_cycles: u8,
    /// Best-effort prose salvaged from a recovery synthesis response that was
    /// rejected for containing tool-call markup. Used as the final answer when
    /// all recovery retries are exhausted, instead of the canned fallback
    /// string, so gathered context is not discarded entirely.
    recovery_rejected_synthesis: Option<String>,
    /// Marks the fresh turn created by an approved plan handoff. This keeps
    /// recovery tool-enabled and lets the turn loop discard stale
    /// "tools are disabled" status responses from the planning turn.
    approved_plan_execution: bool,
    approved_plan_recovery_retries: u8,
    /// Set when the planning interview tool is permanently unavailable. The
    /// tool batch consumes this flag after all tool responses have been
    /// appended so the recovery directive is not interleaved with a batch.
    interview_denial_recovery_pending: bool,
    /// Set when the preflight validation circuit breaker trips. The tool
    /// batch consumes this flag after all tool responses (including drained
    /// skipped-call responses) have been appended, then arms a tool-free
    /// recovery pass so the model can synthesize a plain-text response
    /// instead of the turn hard-blocking and silently dropping an approved
    /// plan build.
    preflight_circuit_recovery_pending: bool,
    pub max_tool_calls: usize,
    pub max_tool_wall_clock: Duration,
    pub max_tool_retries: u32,
    /// Tracks consecutive relaxed continuation decisions. If this exceeds
    /// `MAX_CONSECUTIVE_RELAXED_CONTINUATIONS`, the turn ends to prevent
    /// infinite loops where the model keeps producing continuation-worthy
    /// text without making actual progress.
    pub consecutive_relaxed_continuations: u32,
}

impl HarnessTurnState {
    #[allow(
        clippy::too_many_arguments,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )]
    pub(crate) fn new(
        run_id: TurnRunId,
        turn_id: TurnId,
        max_tool_calls: usize,
        max_tool_wall_clock_secs: u64,
        max_tool_retries: u32,
    ) -> Self {
        Self {
            run_id,
            turn_id,
            phase: TurnPhase::Preparing,
            turn_started_at: Instant::now(),
            wait_started_at: None,
            excluded_wait_duration: Duration::ZERO,
            tool_calls: 0,
            requested_tool_calls: 0,
            admitted_tool_calls: 0,
            failed_tool_calls: 0,
            denied_tool_calls: 0,
            preflight_failures: 0,
            reused_results: 0,
            spooled_results: 0,
            raw_spooled_bytes: 0,
            model_visible_output_bytes: 0,
            recovery_activations: 0,
            blocked_tool_calls: 0,
            consecutive_blocked_tool_calls: 0,
            consecutive_preflight_failures: 0,
            assistant_text_responses_in_turn: 0,
            final_response_rendered: false,
            final_response_event_emitted: false,
            streamed_response_event_emitted: false,
            final_response_was_fallback: false,
            consecutive_spool_chunk_reads: 0,
            consecutive_same_shell_command_runs: 0,
            last_shell_command_signature: None,
            consecutive_same_file_read_family_calls: 0,
            last_file_read_family_signature: None,
            file_read_path_counts: HashMap::new(),
            seen_successful_readonly_signatures: HashSet::new(),
            streamed_tool_call_item_ids: HashMap::new(),
            stop_hook_active: false,
            seen_task_tracker_create_signatures: HashSet::new(),
            replaceable_task_tracker_block: None,
            recently_written_files: HashSet::new(),
            tool_budget_warning_emitted: false,
            tool_budget_exhausted_emitted: false,
            wall_clock_exhausted_emitted: false,
            wall_clock_directive_pending: false,
            tool_budget_directive_pending: false,
            recovery_reason: None,
            recovery_phase: RecoveryPhase::Inactive,
            recovery_mode: None,
            recovery_retry_count: 0,
            post_tool_compaction_pending: false,
            post_tool_context_capacity_failure: false,
            post_tool_context_compaction_failed: false,
            post_tool_tool_enabled_retry_used: false,
            post_tool_recovery_cycles: 0,
            recovery_rejected_synthesis: None,
            approved_plan_execution: false,
            approved_plan_recovery_retries: 0,
            interview_denial_recovery_pending: false,
            preflight_circuit_recovery_pending: false,
            max_tool_calls,
            max_tool_wall_clock: Duration::from_secs(max_tool_wall_clock_secs),
            max_tool_retries,
            consecutive_relaxed_continuations: 0,
        }
    }

    pub(crate) fn has_tool_call_budget(&self) -> bool {
        self.max_tool_calls > 0
    }

    pub(crate) fn tool_budget_exhausted(&self) -> bool {
        self.has_tool_call_budget() && self.tool_calls >= self.max_tool_calls
    }

    pub(crate) fn tool_budget_exhaustion(&self) -> Option<ToolBudgetExhaustion> {
        self.tool_budget_exhausted().then_some(ToolBudgetExhaustion {
            used: self.tool_calls,
            max: self.max_tool_calls,
            remaining: self.remaining_tool_calls(),
        })
    }

    pub(crate) fn wall_clock_exhausted(&self) -> bool {
        self.effective_wall_clock_elapsed() >= self.max_tool_wall_clock
    }

    fn effective_wall_clock_elapsed(&self) -> Duration {
        let elapsed = self.turn_started_at.elapsed();
        let active_wait = self.wait_started_at.map(|started| started.elapsed()).unwrap_or(Duration::ZERO);
        elapsed.saturating_sub(self.excluded_wait_duration.saturating_add(active_wait))
    }

    /// Pause ordinary turn wall-clock accounting around an explicit command
    /// wait. This does not alter tool ceilings or cancellation behavior.
    pub(crate) fn begin_long_running_command_wait(&mut self) {
        if self.wait_started_at.is_none() {
            self.wait_started_at = Some(Instant::now());
        }
    }

    pub(crate) fn end_long_running_command_wait(&mut self) {
        if let Some(started) = self.wait_started_at.take() {
            self.excluded_wait_duration = self.excluded_wait_duration.saturating_add(started.elapsed());
        }
    }

    pub(crate) fn wall_clock_budget_exhaustion(&self) -> Option<ToolWallClockExhaustion> {
        self.wall_clock_exhausted()
            .then_some(ToolWallClockExhaustion { max_secs: self.max_tool_wall_clock.as_secs() })
    }

    pub(crate) fn record_tool_call(&mut self) {
        self.tool_calls = self.tool_calls.saturating_add(1);
    }

    pub(crate) fn record_requested_tool_calls(&mut self, count: usize) {
        self.requested_tool_calls = self
            .requested_tool_calls
            .saturating_add(u32::try_from(count).unwrap_or(u32::MAX));
    }

    pub(crate) fn record_admitted_tool_call(&mut self) {
        self.admitted_tool_calls = self.admitted_tool_calls.saturating_add(1);
    }

    pub(crate) fn record_failed_tool_call(&mut self) {
        self.failed_tool_calls = self.failed_tool_calls.saturating_add(1);
    }

    pub(crate) fn record_denied_tool_call(&mut self) {
        self.denied_tool_calls = self.denied_tool_calls.saturating_add(1);
    }

    pub(crate) fn record_tool_output_metrics(
        &mut self,
        reused: bool,
        spooled: bool,
        raw_spooled_bytes: u64,
        model_visible_output_bytes: usize,
    ) {
        if reused {
            self.reused_results = self.reused_results.saturating_add(1);
        }
        if spooled {
            self.spooled_results = self.spooled_results.saturating_add(1);
        }
        self.raw_spooled_bytes = self.raw_spooled_bytes.saturating_add(raw_spooled_bytes);
        self.model_visible_output_bytes = self
            .model_visible_output_bytes
            .saturating_add(u64::try_from(model_visible_output_bytes).unwrap_or(u64::MAX));
    }

    pub(crate) fn snapshot_turn_diagnostics(
        &self,
        usage: vtcode_core::exec::events::Usage,
        low_signal_tool_calls: u32,
    ) -> vtcode_core::core::agent::snapshots::SnapshotTurnDiagnostics {
        vtcode_core::core::agent::snapshots::SnapshotTurnDiagnostics {
            usage,
            requested_tool_calls: self.requested_tool_calls,
            admitted_tool_calls: self.admitted_tool_calls,
            failed_tool_calls: self.failed_tool_calls,
            denied_tool_calls: self.denied_tool_calls,
            preflight_failures: self.preflight_failures,
            reused_results: self.reused_results,
            spooled_results: self.spooled_results,
            raw_spooled_bytes: self.raw_spooled_bytes,
            model_visible_output_bytes: self.model_visible_output_bytes,
            low_signal_tool_calls,
            recovery_activations: self.recovery_activations,
            ..Default::default()
        }
    }

    pub(crate) fn record_tool_call_with_warning(&mut self, threshold: f64) -> Option<ToolBudgetWarning> {
        self.record_tool_call();
        if !self.should_emit_tool_budget_warning(threshold) {
            return None;
        }

        let warning = ToolBudgetWarning {
            used: self.tool_calls,
            max: self.max_tool_calls,
            remaining: self.remaining_tool_calls(),
        };
        self.mark_tool_budget_warning_emitted();
        Some(warning)
    }

    pub(crate) fn record_tool_call_with_default_warning(&mut self) -> Option<ToolBudgetWarning> {
        self.record_tool_call_with_warning(TOOL_BUDGET_WARNING_THRESHOLD)
    }

    /// Record that the agent emitted a text response (no tool calls) in this
    /// turn.  The turn loop also counts existing assistant messages in
    /// `working_history` for the anti-runaway guard; this method exists
    /// for symmetry with the other `record_*` helpers and is incremented
    /// alongside the in-message tracking.
    pub(crate) fn record_assistant_text_response(&mut self) -> u32 {
        self.assistant_text_responses_in_turn = self.assistant_text_responses_in_turn.saturating_add(1);
        self.assistant_text_responses_in_turn
    }

    pub(crate) fn record_tool_budget_exhaustion_notice(&mut self) -> Option<ToolBudgetExhaustionNotice> {
        let exhaustion = self.tool_budget_exhaustion()?;
        let first_notice = !self.tool_budget_exhausted_emitted;
        if first_notice {
            self.mark_tool_budget_exhausted_emitted();
            self.tool_budget_directive_pending = true;
        }
        Some(ToolBudgetExhaustionNotice { exhaustion, first_notice })
    }

    /// Consume the pending tool-call-budget synthesis-directive flag. Returns
    /// `true` exactly once per turn (after the batch where exhaustion first
    /// fired). Mirrors `take_wall_clock_directive_pending`.
    pub(crate) fn take_tool_budget_directive_pending(&mut self) -> bool {
        std::mem::take(&mut self.tool_budget_directive_pending)
    }

    /// Record a wall-clock-budget rejection for the current tool call.
    ///
    /// Returns `None` when the budget is not exhausted. On the first exhausted
    /// call it flags `first_notice` (so the full policy message is emitted once)
    /// and arms `wall_clock_directive_pending` so the handler pushes a single
    /// "synthesize now" system directive *after* the tool batch completes.
    pub(crate) fn record_wall_clock_exhaustion_notice(&mut self) -> Option<ToolWallClockExhaustionNotice> {
        let exhaustion = self.wall_clock_budget_exhaustion()?;
        let first_notice = !self.wall_clock_exhausted_emitted;
        if first_notice {
            self.wall_clock_exhausted_emitted = true;
            self.wall_clock_directive_pending = true;
        }
        Some(ToolWallClockExhaustionNotice { exhaustion, first_notice })
    }

    /// Consume the pending wall-clock synthesis-directive flag. Returns `true`
    /// exactly once per turn (after the batch where exhaustion first fired).
    pub(crate) fn take_wall_clock_directive_pending(&mut self) -> bool {
        std::mem::take(&mut self.wall_clock_directive_pending)
    }

    pub(crate) fn record_blocked_tool_call(&mut self) -> usize {
        self.blocked_tool_calls = self.blocked_tool_calls.saturating_add(1);
        self.consecutive_blocked_tool_calls = self.consecutive_blocked_tool_calls.saturating_add(1);
        self.consecutive_blocked_tool_calls
    }

    pub(crate) fn reset_blocked_tool_call_streak(&mut self) {
        self.consecutive_blocked_tool_calls = 0;
    }

    pub(crate) fn record_preflight_failure(&mut self) -> usize {
        self.consecutive_preflight_failures = self.consecutive_preflight_failures.saturating_add(1);
        self.preflight_failures = self.preflight_failures.saturating_add(1);
        self.consecutive_preflight_failures
    }

    pub(crate) fn reset_preflight_failure_streak(&mut self) {
        self.consecutive_preflight_failures = 0;
    }

    pub(crate) fn tool_budget_usage_ratio(&self) -> f64 {
        if !self.has_tool_call_budget() {
            0.0
        } else {
            self.tool_calls as f64 / self.max_tool_calls as f64
        }
    }

    pub(crate) fn remaining_tool_calls(&self) -> usize {
        self.max_tool_calls.saturating_sub(self.tool_calls)
    }

    pub(crate) fn should_emit_tool_budget_warning(&self, threshold: f64) -> bool {
        self.has_tool_call_budget() && !self.tool_budget_warning_emitted && self.tool_budget_usage_ratio() >= threshold
    }

    pub(crate) fn mark_tool_budget_warning_emitted(&mut self) {
        self.tool_budget_warning_emitted = true;
    }

    pub(crate) fn mark_tool_budget_exhausted_emitted(&mut self) {
        self.tool_budget_exhausted_emitted = true;
    }

    pub(crate) fn activate_recovery(&mut self, reason: impl Into<String>) {
        self.activate_recovery_with_mode(reason, RecoveryMode::ToolFreeSynthesis);
    }

    pub(crate) fn activate_recovery_with_mode(&mut self, reason: impl Into<String>, mode: RecoveryMode) {
        if matches!(self.recovery_phase, RecoveryPhase::Inactive) {
            self.recovery_activations = self.recovery_activations.saturating_add(1);
            self.recovery_reason = Some(reason.into());
            self.recovery_phase = RecoveryPhase::Pending;
            self.recovery_mode = Some(mode);
            self.recovery_retry_count = 0;
        }
    }

    /// Arm the single tool-enabled retry used after a provider failure follows
    /// successful tool execution. The runloop consumes the
    /// compaction flag before consuming the recovery pass, so the retry sees
    /// the compacted prefix plus the current request and tool outputs.
    pub(crate) fn arm_post_tool_tool_enabled_retry(
        &mut self,
        reason: impl Into<String>,
        context_capacity_failure: bool,
    ) -> bool {
        if !matches!(self.recovery_phase, RecoveryPhase::Inactive) {
            return false;
        }

        self.recovery_activations = self.recovery_activations.saturating_add(1);
        self.recovery_reason = Some(reason.into());
        self.recovery_phase = RecoveryPhase::Pending;
        self.recovery_mode = Some(RecoveryMode::ToolEnabledRetry);
        self.recovery_retry_count = 0;
        self.post_tool_compaction_pending = true;
        self.post_tool_context_capacity_failure = context_capacity_failure;
        self.post_tool_tool_enabled_retry_used = true;
        true
    }

    pub(crate) fn is_recovery_active(&self) -> bool {
        matches!(self.recovery_phase, RecoveryPhase::Pending | RecoveryPhase::InPass)
    }

    pub(crate) fn recovery_reason(&self) -> Option<&str> {
        self.recovery_reason.as_deref()
    }

    pub(crate) fn recovery_pass_used(&self) -> bool {
        matches!(self.recovery_phase, RecoveryPhase::InPass | RecoveryPhase::Completed)
    }

    #[cfg(test)]
    fn recovery_mode(&self) -> Option<RecoveryMode> {
        self.recovery_mode
    }

    /// Switch to tool-free synthesis mode and reset the recovery phase back to
    /// `Pending` so the next loop iteration can consume it.
    ///
    /// Unlike `activate_recovery_with_mode` (which is a guarded no-op once a
    /// pass is in flight), this unconditionally forces the phase to `Pending`,
    /// covering `Inactive`, `InPass`, and `Completed`. This is required
    /// because the post-tool follow-up failure path runs from a *non-recovery*
    /// turn (phase == `Inactive`): `activate_recovery_with_mode` would set the
    /// reason and mode but leave the phase as `Inactive`, so
    /// `consume_recovery_pass()` would return `false`, `tool_free_recovery`
    /// would evaluate to `false`, and tools would never be disabled at the API
    /// level.
    ///
    /// When transitioning from `Inactive`, this also resets the retry counter
    /// and seeds a default `recovery_reason` (mirroring
    /// `activate_recovery_with_mode`) so the `[Recovery Mode]` request block
    /// reports why recovery was engaged.
    ///
    /// Returns `true` when the phase actually changed.
    pub(crate) fn switch_to_tool_free_recovery(&mut self) -> bool {
        let was_inactive = matches!(self.recovery_phase, RecoveryPhase::Inactive);
        self.recovery_mode = Some(RecoveryMode::ToolFreeSynthesis);
        let changed = !matches!(self.recovery_phase, RecoveryPhase::Pending);
        self.recovery_phase = RecoveryPhase::Pending;
        if was_inactive {
            self.recovery_activations = self.recovery_activations.saturating_add(1);
            self.recovery_retry_count = 0;
            if self.recovery_reason.is_none() {
                self.recovery_reason = Some("post-tool follow-up failure".to_string());
            }
        }
        changed
    }

    /// Arm the bounded tool-free plan synthesis fallback used when
    /// `request_user_input` is permanently unavailable in the current
    /// runtime. The directive is flushed after the current tool batch so
    /// provider message ordering remains valid.
    pub(crate) fn arm_interview_denial_recovery(&mut self) {
        self.interview_denial_recovery_pending = true;
    }

    pub(crate) fn take_interview_denial_recovery(&mut self) -> bool {
        std::mem::take(&mut self.interview_denial_recovery_pending)
    }

    pub(crate) fn interview_denial_recovery_pending(&self) -> bool {
        self.interview_denial_recovery_pending
    }

    /// Arm the preflight circuit-breaker recovery so the tool batch can flush
    /// its synthesis directive after all tool responses land.
    pub(crate) fn arm_preflight_circuit_recovery(&mut self) {
        self.preflight_circuit_recovery_pending = true;
    }

    pub(crate) fn take_preflight_circuit_recovery(&mut self) -> bool {
        std::mem::take(&mut self.preflight_circuit_recovery_pending)
    }

    pub(crate) fn recovery_is_tool_free(&self) -> bool {
        matches!(self.recovery_mode, Some(RecoveryMode::ToolFreeSynthesis))
    }

    #[cfg(test)]
    pub(crate) fn post_tool_compaction_pending(&self) -> bool {
        self.post_tool_compaction_pending
    }

    pub(crate) fn take_post_tool_compaction_pending(&mut self) -> bool {
        std::mem::take(&mut self.post_tool_compaction_pending)
    }

    pub(crate) fn post_tool_context_capacity_failure(&self) -> bool {
        self.post_tool_context_capacity_failure
    }

    pub(crate) fn mark_post_tool_context_compaction_failed(&mut self) {
        self.post_tool_context_compaction_failed = true;
    }

    pub(crate) fn post_tool_context_compaction_failed(&self) -> bool {
        self.post_tool_context_compaction_failed
    }

    pub(crate) fn post_tool_tool_enabled_retry_used(&self) -> bool {
        self.post_tool_tool_enabled_retry_used
    }

    pub(crate) fn set_approved_plan_execution(&mut self, active: bool) {
        self.approved_plan_execution = active;
        self.approved_plan_recovery_retries = 0;
    }

    pub(crate) fn final_response_rendered(&self) -> bool {
        self.final_response_rendered
    }

    pub(crate) fn final_response_event_emitted(&self) -> bool {
        self.final_response_event_emitted
    }

    pub(crate) fn mark_final_response_rendered(&mut self) {
        self.final_response_rendered = true;
    }

    pub(crate) fn mark_final_response_event_emitted(&mut self) {
        self.final_response_event_emitted = true;
    }

    pub(crate) fn mark_streamed_response_event_emitted(&mut self) {
        self.streamed_response_event_emitted = true;
    }

    pub(crate) fn reset_streamed_response_event_emitted(&mut self) {
        self.streamed_response_event_emitted = false;
    }

    pub(crate) fn streamed_response_event_emitted(&self) -> bool {
        self.streamed_response_event_emitted
    }

    pub(crate) fn mark_final_response_fallback(&mut self) {
        self.final_response_was_fallback = true;
    }

    pub(crate) fn final_response_was_fallback(&self) -> bool {
        self.final_response_was_fallback
    }

    pub(crate) fn is_approved_plan_execution(&self) -> bool {
        self.approved_plan_execution
    }

    pub(crate) fn approved_plan_recovery_retries(&self) -> u8 {
        self.approved_plan_recovery_retries
    }

    pub(crate) fn record_approved_plan_recovery_retry(&mut self) {
        self.approved_plan_recovery_retries = self.approved_plan_recovery_retries.saturating_add(1);
    }

    pub(crate) fn consume_recovery_pass(&mut self) -> bool {
        if !matches!(self.recovery_phase, RecoveryPhase::Pending) {
            return false;
        }
        self.recovery_phase = RecoveryPhase::InPass;
        true
    }

    pub(crate) fn finish_recovery_pass(&mut self) -> bool {
        if !matches!(self.recovery_phase, RecoveryPhase::InPass) {
            return false;
        }
        self.recovery_phase = RecoveryPhase::Completed;
        true
    }

    /// Retry the recovery pass by resetting the phase back to `Pending`
    /// so the next loop iteration re-enters tool-free recovery mode.
    /// Increments the retry counter; the caller is responsible for checking
    /// `recovery_retry_count()` against its own limit.
    /// Only works if a recovery pass has been consumed (phase is InPass or Completed).
    pub(crate) fn retry_recovery_pass(&mut self) -> bool {
        if matches!(self.recovery_phase, RecoveryPhase::InPass | RecoveryPhase::Completed) {
            self.recovery_phase = RecoveryPhase::Pending;
            self.recovery_retry_count += 1;
            true
        } else {
            false
        }
    }

    pub(crate) fn recovery_retry_count(&self) -> u8 {
        self.recovery_retry_count
    }

    /// Record best-effort prose salvaged from a rejected recovery synthesis
    /// response. Later rejections overwrite earlier ones (the latest attempt
    /// is the most complete).
    pub(crate) fn record_recovery_rejected_synthesis(&mut self, text: String) {
        if !text.trim().is_empty() {
            self.recovery_rejected_synthesis = Some(text);
        }
    }

    pub(crate) fn take_recovery_rejected_synthesis(&mut self) -> Option<String> {
        self.recovery_rejected_synthesis.take()
    }

    pub(crate) fn post_tool_recovery_cycles(&self) -> u8 {
        self.post_tool_recovery_cycles
    }

    /// Increment the tool-free post-tool recovery cycle counter. Returns the new value.
    pub(crate) fn increment_post_tool_recovery_cycle(&mut self) -> u8 {
        self.post_tool_recovery_cycles = self.post_tool_recovery_cycles.saturating_add(1);
        self.post_tool_recovery_cycles
    }

    pub(crate) fn record_spool_chunk_read(&mut self) -> usize {
        self.consecutive_spool_chunk_reads = self.consecutive_spool_chunk_reads.saturating_add(1);
        self.consecutive_spool_chunk_reads
    }

    pub(crate) fn reset_spool_chunk_read_streak(&mut self) {
        self.consecutive_spool_chunk_reads = 0;
    }

    pub(crate) fn record_shell_command_run(&mut self, signature: String) -> usize {
        if self.last_shell_command_signature.as_deref() == Some(signature.as_str()) {
            self.consecutive_same_shell_command_runs = self.consecutive_same_shell_command_runs.saturating_add(1);
        } else {
            self.last_shell_command_signature = Some(signature);
            self.consecutive_same_shell_command_runs = 1;
        }

        self.consecutive_same_shell_command_runs
    }

    pub(crate) fn reset_shell_command_run_streak(&mut self) {
        self.last_shell_command_signature = None;
        self.consecutive_same_shell_command_runs = 0;
    }

    pub(crate) fn record_file_read_family_call(&mut self, signature: String) -> usize {
        if self.last_file_read_family_signature.as_deref() == Some(signature.as_str()) {
            self.consecutive_same_file_read_family_calls =
                self.consecutive_same_file_read_family_calls.saturating_add(1);
        } else {
            self.last_file_read_family_signature = Some(signature);
            self.consecutive_same_file_read_family_calls = 1;
        }

        self.consecutive_same_file_read_family_calls
    }

    pub(crate) fn reset_file_read_family_streak(&mut self) {
        self.last_file_read_family_signature = None;
        self.consecutive_same_file_read_family_calls = 0;
    }

    /// Record a read of `path` and return the total count of reads for that
    /// path this turn. Independent of slice (offset/limit/raw) — catches
    /// paginated reads of the same file that the slice-aware family key lets
    /// through.
    pub(crate) fn record_file_read_path_call(&mut self, path: String) -> usize {
        let count = self.file_read_path_counts.entry(path).or_insert(0);
        *count = count.saturating_add(1);
        *count
    }

    #[cfg(test)]
    fn reset_file_read_path_counts(&mut self) {
        self.file_read_path_counts.clear();
    }

    pub(crate) fn record_written_file(&mut self, path: &str) {
        self.recently_written_files.insert(path.to_string());
    }

    pub(crate) fn was_recently_written(&self, path: &str) -> bool {
        self.recently_written_files.contains(path)
    }

    pub(crate) fn record_task_tracker_create_signature(&mut self, signature: String) -> bool {
        self.seen_task_tracker_create_signatures.insert(signature)
    }

    pub(crate) fn clear_task_tracker_create_signatures(&mut self) {
        self.seen_task_tracker_create_signatures.clear();
    }

    pub(crate) fn record_successful_readonly_signature(&mut self, signature: String) -> bool {
        self.seen_successful_readonly_signatures.insert(signature)
    }

    pub(crate) fn has_successful_readonly_signature(&self, signature: &str) -> bool {
        self.seen_successful_readonly_signatures.contains(signature)
    }

    pub(crate) fn remember_streamed_tool_call_items<I>(&mut self, items: I)
    where
        I: IntoIterator<Item = (String, String)>,
    {
        self.streamed_tool_call_item_ids.extend(items);
    }

    pub(crate) fn take_streamed_tool_call_item_id(&mut self, tool_call_id: &str) -> Option<String> {
        self.streamed_tool_call_item_ids.remove(tool_call_id)
    }

    pub(crate) fn replaceable_task_tracker_count(&self) -> Option<usize> {
        let lines = self.replaceable_task_tracker_block.as_ref()?;
        vtcode_core::utils::transcript::tail_matches(lines).then_some(lines.len())
    }

    pub(crate) fn remember_task_tracker_block(&mut self, lines: Vec<String>) {
        self.replaceable_task_tracker_block = (!lines.is_empty()).then_some(lines);
    }

    pub(crate) fn set_phase(&mut self, phase: TurnPhase) {
        self.phase = phase;
    }

    pub(crate) fn execution_snapshot(&self) -> TurnExecutionSnapshot {
        TurnExecutionSnapshot {
            run_id: self.run_id.0.clone(),
            turn_id: self.turn_id.0.clone(),
            phase: self.phase.into(),
            max_tool_calls: self.max_tool_calls,
            max_tool_wall_clock_secs: self.max_tool_wall_clock.as_secs(),
            max_tool_retries: self.max_tool_retries,
        }
    }
}

pub(crate) struct RunLoopContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub tool_registry: &'a mut ToolRegistry,
    pub tool_result_cache: &'a Arc<RwLock<ToolResultCache>>,
    pub tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
    pub permissions_state: &'a Arc<RwLock<PermissionsConfig>>,
    pub decision_ledger: &'a Arc<RwLock<DecisionTracker>>,
    pub session_stats: &'a mut SessionStats,
    pub plan_session: &'a mut PlanningWorkflowSessionState,
    pub mcp_panel_state: &'a mut McpPanelState,
    pub approval_recorder: &'a ApprovalRecorder,
    pub session: &'a mut InlineSession,
    pub safety_validator: Option<&'a Arc<ToolCallSafetyValidator>>,
    pub traj: &'a TrajectoryLogger,
    pub harness_state: &'a mut HarnessTurnState,
    pub harness_emitter: Option<&'a HarnessEventEmitter>,
    pub auto_permission: Option<AutoPermissionRuntimeContext<'a>>,
    /// Whether ordinary confirmation prompts are bypassed for this turn.
    pub skip_confirmations: bool,
    /// Whether the session is operating under full-auto policy.
    pub full_auto: bool,
    pub active_agent_permissions: Option<&'a AgentPermissionsConfig>,
    /// Name of the currently active agent, if known
    pub agent_name: Option<String>,
    /// Configured execution agent used when a plan is approved from the
    /// dedicated planning agent without a previous agent to restore.
    pub default_primary_agent: Option<String>,
    /// Whether the current agent is a subagent
    pub is_subagent: bool,
}

pub(crate) struct AutoPermissionRuntimeContext<'a> {
    pub config: &'a CoreAgentConfig,
    pub vt_cfg: Option<&'a VTCodeConfig>,
    pub provider_client: &'a mut dyn uni::LLMProvider,
    pub working_history: &'a [uni::Message],
}

impl<'a> RunLoopContext<'a> {
    #[expect(
        clippy::too_many_arguments,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
    pub(crate) fn new(
        renderer: &'a mut AnsiRenderer,
        handle: &'a InlineHandle,
        tool_registry: &'a mut ToolRegistry,
        _tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
        tool_result_cache: &'a Arc<RwLock<ToolResultCache>>,
        tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
        permissions_state: &'a Arc<RwLock<PermissionsConfig>>,
        decision_ledger: &'a Arc<RwLock<DecisionTracker>>,
        session_stats: &'a mut SessionStats,
        plan_session: &'a mut PlanningWorkflowSessionState,
        mcp_panel_state: &'a mut McpPanelState,
        approval_recorder: &'a ApprovalRecorder,
        session: &'a mut InlineSession,
        safety_validator: Option<&'a Arc<ToolCallSafetyValidator>>,
        traj: &'a TrajectoryLogger,
        harness_state: &'a mut HarnessTurnState,
        harness_emitter: Option<&'a HarnessEventEmitter>,
    ) -> Self {
        Self::new_with_auto_permission_context(
            renderer,
            handle,
            tool_registry,
            _tools,
            tool_result_cache,
            tool_permission_cache,
            permissions_state,
            decision_ledger,
            session_stats,
            plan_session,
            mcp_panel_state,
            approval_recorder,
            session,
            safety_validator,
            traj,
            harness_state,
            harness_emitter,
            None,
            false,
            false,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
    pub(crate) fn new_with_auto_permission_context(
        renderer: &'a mut AnsiRenderer,
        handle: &'a InlineHandle,
        tool_registry: &'a mut ToolRegistry,
        _tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
        tool_result_cache: &'a Arc<RwLock<ToolResultCache>>,
        tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
        permissions_state: &'a Arc<RwLock<PermissionsConfig>>,
        decision_ledger: &'a Arc<RwLock<DecisionTracker>>,
        session_stats: &'a mut SessionStats,
        plan_session: &'a mut PlanningWorkflowSessionState,
        mcp_panel_state: &'a mut McpPanelState,
        approval_recorder: &'a ApprovalRecorder,
        session: &'a mut InlineSession,
        safety_validator: Option<&'a Arc<ToolCallSafetyValidator>>,
        traj: &'a TrajectoryLogger,
        harness_state: &'a mut HarnessTurnState,
        harness_emitter: Option<&'a HarnessEventEmitter>,
        auto_permission: Option<AutoPermissionRuntimeContext<'a>>,
        skip_confirmations: bool,
        full_auto: bool,
    ) -> Self {
        Self {
            renderer,
            handle,
            tool_registry,
            tool_result_cache,
            tool_permission_cache,
            permissions_state,
            decision_ledger,
            session_stats,
            plan_session,
            mcp_panel_state,
            approval_recorder,
            session,
            safety_validator,
            traj,
            harness_state,
            harness_emitter,
            auto_permission,
            skip_confirmations,
            full_auto,
            active_agent_permissions: None,
            agent_name: None,
            default_primary_agent: None,
            is_subagent: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use hashbrown::HashSet;

    use super::{
        CrossTurnTracker, HarnessTurnState, RecoveryMode, TOOL_BUDGET_WARNING_THRESHOLD, ToolBudgetExhaustion,
        ToolBudgetExhaustionNotice, ToolBudgetWarning, ToolWallClockExhaustion, ToolWallClockExhaustionNotice,
        TurnExecutionPhase, TurnId, TurnPhase, TurnRunId,
    };

    #[test]
    fn harness_state_tracks_phase_transitions() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 2, 10, 1);

        // Verify that run_id and turn_id are accessible
        assert_eq!(state.run_id.0, "run-1");
        assert_eq!(state.turn_id.0, "turn-1");

        assert_eq!(state.phase, TurnPhase::Preparing);
        state.set_phase(TurnPhase::Requesting);
        assert_eq!(state.phase, TurnPhase::Requesting);
        state.set_phase(TurnPhase::ExecutingTools);
        assert_eq!(state.phase, TurnPhase::ExecutingTools);
        state.set_phase(TurnPhase::Finalizing);
        assert_eq!(state.phase, TurnPhase::Finalizing);
    }

    #[test]
    fn harness_state_tracks_spool_chunk_read_streak() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 2, 10, 1);

        assert_eq!(state.record_spool_chunk_read(), 1);
        assert_eq!(state.record_spool_chunk_read(), 2);
        state.reset_spool_chunk_read_streak();
        assert_eq!(state.record_spool_chunk_read(), 1);
    }

    #[test]
    fn harness_state_tracks_budget_warning_threshold_once() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        assert!(!state.should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD));
        state.record_tool_call(); // 1/4
        assert!(!state.should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD));
        state.record_tool_call(); // 2/4
        assert!(!state.should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD));
        state.record_tool_call(); // 3/4 => 75%
        assert!(state.should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD));
        state.mark_tool_budget_warning_emitted();
        assert!(!state.should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD));
        assert_eq!(state.remaining_tool_calls(), 1);
    }

    #[test]
    fn harness_state_records_budget_warning_once_via_helper() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        assert_eq!(state.record_tool_call_with_default_warning(), None);
        assert_eq!(state.record_tool_call_with_default_warning(), None);
        assert_eq!(
            state.record_tool_call_with_default_warning(),
            Some(ToolBudgetWarning { used: 3, max: 4, remaining: 1 })
        );
        assert_eq!(state.record_tool_call_with_default_warning(), None);
    }

    #[test]
    fn tool_budget_warning_system_message_matches_contract() {
        assert_eq!(
            ToolBudgetWarning { used: 3, max: 4, remaining: 1 }.system_message(),
            "Tool-call budget warning: 3/4 used; 1 remaining for this turn. Use targeted extraction/batching before additional tool calls."
        );
    }

    #[test]
    fn harness_state_records_budget_exhaustion_notice_once_via_helper() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 1, 10, 1);

        assert!(!state.tool_budget_exhausted());
        assert!(!state.tool_budget_exhausted_emitted);
        state.record_tool_call();
        assert!(state.tool_budget_exhausted());
        assert_eq!(
            state.record_tool_budget_exhaustion_notice(),
            Some(ToolBudgetExhaustionNotice {
                exhaustion: ToolBudgetExhaustion { used: 1, max: 1, remaining: 0 },
                first_notice: true,
            })
        );
        assert!(state.tool_budget_exhausted_emitted);
        assert_eq!(
            state.record_tool_budget_exhaustion_notice(),
            Some(ToolBudgetExhaustionNotice {
                exhaustion: ToolBudgetExhaustion { used: 1, max: 1, remaining: 0 },
                first_notice: false,
            })
        );
    }

    #[test]
    fn tool_budget_exhaustion_synthesis_directive_matches_contract() {
        assert_eq!(
            ToolBudgetExhaustion { used: 4, max: 4, remaining: 0 }.synthesis_directive_message(),
            "Tool-call budget exhausted for this turn (4/4). Tools are disabled for the rest of this turn. Do NOT emit more tool calls. Synthesize your final answer now from the tool outputs already gathered in this conversation."
        );
    }

    #[test]
    fn tool_budget_exhaustion_notice_arms_synthesis_directive_once() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 1, 600, 3);
        state.record_tool_call();
        assert!(state.record_tool_budget_exhaustion_notice().is_some());
        assert!(state.take_tool_budget_directive_pending());
        // Second rejected call in the same turn must not re-arm the directive.
        assert!(state.record_tool_budget_exhaustion_notice().is_some());
        assert!(!state.take_tool_budget_directive_pending());
    }

    #[test]
    fn tool_wall_clock_exhaustion_policy_violation_message_matches_contract() {
        assert_eq!(
            ToolWallClockExhaustion { max_secs: 600 }.policy_violation_message(),
            "Policy violation: exceeded tool wall clock budget (600s)"
        );
    }

    #[test]
    fn harness_state_treats_zero_tool_budget_as_unlimited() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 0, 10, 1);

        for _ in 0..8 {
            state.record_tool_call();
        }

        assert!(!state.has_tool_call_budget());
        assert!(!state.tool_budget_exhausted());
        assert_eq!(state.tool_budget_exhaustion(), None);
        assert!(!state.should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD));
    }

    #[test]
    fn harness_state_reports_wall_clock_budget_exhaustion() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        assert_eq!(state.wall_clock_budget_exhaustion(), None);
        state.turn_started_at = Instant::now().checked_sub(Duration::from_secs(11)).unwrap();
        assert_eq!(state.wall_clock_budget_exhaustion(), Some(ToolWallClockExhaustion { max_secs: 10 }));
    }

    #[test]
    fn harness_state_records_wall_clock_exhaustion_notice_once() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        // Not exhausted yet: no notice, no pending directive.
        assert_eq!(state.record_wall_clock_exhaustion_notice(), None);
        assert!(!state.take_wall_clock_directive_pending());

        // Simulate the wall-clock budget elapsing.
        state.turn_started_at = Instant::now().checked_sub(Duration::from_secs(11)).unwrap();

        // First rejection: first_notice=true and arms the directive.
        assert_eq!(
            state.record_wall_clock_exhaustion_notice(),
            Some(ToolWallClockExhaustionNotice {
                exhaustion: ToolWallClockExhaustion { max_secs: 10 },
                first_notice: true,
            })
        );
        assert!(state.wall_clock_exhausted_emitted);

        // Subsequent rejections in the same batch: first_notice=false.
        assert_eq!(
            state.record_wall_clock_exhaustion_notice(),
            Some(ToolWallClockExhaustionNotice {
                exhaustion: ToolWallClockExhaustion { max_secs: 10 },
                first_notice: false,
            })
        );

        // The directive is consumed exactly once.
        assert!(state.take_wall_clock_directive_pending());
        assert!(!state.take_wall_clock_directive_pending());
    }

    #[test]
    fn tool_wall_clock_exhaustion_directive_messages_match_contract() {
        let exhaustion = ToolWallClockExhaustion { max_secs: 600 };
        assert_eq!(exhaustion.skipped_call_message(), "Tool wall-clock budget exhausted for this turn; call skipped.");
        assert_eq!(
            exhaustion.synthesis_directive_message(),
            "Tool wall-clock budget exhausted for this turn (600s). Tools are disabled for the rest of this turn. Do NOT emit more tool calls. Synthesize your final answer now from the tool outputs already gathered in this conversation."
        );
    }

    #[test]
    fn harness_state_tracks_blocked_call_streak() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        assert_eq!(state.blocked_tool_calls, 0);
        assert_eq!(state.record_blocked_tool_call(), 1);
        assert_eq!(state.record_blocked_tool_call(), 2);
        assert_eq!(state.blocked_tool_calls, 2);
        state.reset_blocked_tool_call_streak();
        assert_eq!(state.consecutive_blocked_tool_calls, 0);
    }

    #[test]
    fn harness_state_tracks_and_resets_preflight_failure_streak() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        assert_eq!(state.record_preflight_failure(), 1);
        assert_eq!(state.record_preflight_failure(), 2);
        state.reset_preflight_failure_streak();
        assert_eq!(state.consecutive_preflight_failures, 0);
    }

    #[test]
    fn harness_state_tracks_recovery_state() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        assert!(!state.is_recovery_active());
        assert!(!state.recovery_pass_used());

        state.activate_recovery("loop detector");
        assert!(state.is_recovery_active());
        assert_eq!(state.recovery_reason(), Some("loop detector"));
        assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolFreeSynthesis));
        assert!(state.recovery_is_tool_free());

        assert!(state.consume_recovery_pass());
        assert!(state.recovery_pass_used());
        assert!(state.finish_recovery_pass());
        assert!(!state.is_recovery_active());
    }

    #[test]
    fn harness_state_consumes_recovery_pass_once() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        assert!(!state.consume_recovery_pass());

        state.activate_recovery("loop detector");
        assert!(state.consume_recovery_pass());
        assert!(!state.consume_recovery_pass());
        assert!(state.recovery_pass_used());
        assert!(state.finish_recovery_pass());
        assert!(!state.finish_recovery_pass());
    }

    #[test]
    fn harness_state_supports_tool_enabled_recovery_retries() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        state.activate_recovery_with_mode("empty response", RecoveryMode::ToolEnabledRetry);

        assert!(state.is_recovery_active());
        assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolEnabledRetry));
        assert!(!state.recovery_is_tool_free());
        assert!(state.consume_recovery_pass());
        assert!(state.finish_recovery_pass());
    }

    #[test]
    fn harness_state_arms_one_post_tool_compaction_retry() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        assert!(state.arm_post_tool_tool_enabled_retry("transient post-tool failure", false));
        assert!(state.is_recovery_active());
        assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolEnabledRetry));
        assert!(!state.recovery_is_tool_free());
        assert!(state.post_tool_compaction_pending());
        assert!(state.post_tool_tool_enabled_retry_used());
        assert!(!state.post_tool_context_capacity_failure());
        assert!(state.take_post_tool_compaction_pending());
        assert!(!state.post_tool_compaction_pending());
        assert!(!state.arm_post_tool_tool_enabled_retry("must remain bounded", false));
    }

    #[test]
    fn harness_state_marks_context_capacity_recovery() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        assert!(state.arm_post_tool_tool_enabled_retry("context limit", true));
        assert!(state.post_tool_context_capacity_failure());
        assert!(!state.post_tool_context_compaction_failed());

        state.mark_post_tool_context_compaction_failed();
        assert!(state.post_tool_context_compaction_failed());
    }

    #[test]
    fn harness_state_switch_to_tool_free_recovery_from_inactive() {
        // Regression guard for the post-tool follow-up infinite loop:
        // `switch_to_tool_free_recovery` must transition `Inactive -> Pending`
        // (not just `InPass` or `Completed` to `Pending`). When a normal
        // (non-recovery)
        // turn's follow-up LLM phase fails, the phase is `Inactive`; if the
        // switch left it there, `consume_recovery_pass()` would return false,
        // `tool_free_recovery` would evaluate to false, and tools would never
        // be disabled at the API level.
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        // Fresh state: recovery is inactive.
        assert!(!state.is_recovery_active());
        assert!(!state.recovery_is_tool_free());

        // Switching from Inactive must engage a tool-free recovery pass.
        assert!(state.switch_to_tool_free_recovery(), "switch from Inactive must report a phase change");
        assert!(state.is_recovery_active(), "phase must be Pending");
        assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolFreeSynthesis));
        assert!(state.recovery_is_tool_free());

        // The pass must be consumable. This is what the turn loop checks to
        // decide `tool_free_recovery = true` and disable tools at the API level.
        assert!(state.consume_recovery_pass(), "consume_recovery_pass must succeed after switch from Inactive");

        // A default recovery reason must be seeded so the [Recovery Mode]
        // request block reports why recovery was engaged.
        assert!(state.recovery_reason().is_some(), "recovery_reason must be seeded when switching from Inactive");
    }

    #[test]
    fn harness_state_switch_to_tool_free_recovery_from_in_pass_keeps_consumable() {
        // Switching from InPass (a pass already in flight) must still reset to
        // Pending so the next loop iteration can consume it.
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        state.activate_recovery_with_mode("empty response", RecoveryMode::ToolEnabledRetry);
        assert!(state.consume_recovery_pass()); // -> InPass
        assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolEnabledRetry));

        assert!(state.switch_to_tool_free_recovery());
        assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolFreeSynthesis));
        assert!(state.consume_recovery_pass(), "pass must be consumable again after switching from InPass");
    }

    #[test]
    fn harness_state_switch_to_tool_free_recovery_from_completed_keeps_consumable() {
        // No-regression guard: switching from Completed must reset to Pending.
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        state.activate_recovery_with_mode("empty response", RecoveryMode::ToolEnabledRetry);
        assert!(state.consume_recovery_pass()); // -> InPass
        assert!(state.finish_recovery_pass()); // -> Completed
        assert!(!state.is_recovery_active());

        assert!(state.switch_to_tool_free_recovery());
        assert!(state.is_recovery_active());
        assert!(state.consume_recovery_pass());
    }

    #[test]
    fn harness_state_switch_to_tool_free_recovery_idempotent_when_pending() {
        // When already Pending, switching reports no phase change but still
        // forces the mode to ToolFreeSynthesis.
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        state.activate_recovery("loop detector");
        assert!(state.is_recovery_active()); // Pending

        assert!(!state.switch_to_tool_free_recovery(), "switch from Pending must report no phase change");
        assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolFreeSynthesis));
        assert!(state.consume_recovery_pass());
    }

    #[test]
    fn harness_state_switch_to_tool_free_recovery_resets_retry_count_from_inactive() {
        // Switching from Inactive resets the retry counter (mirrors
        // activate_recovery_with_mode) so any stale count does not
        // prematurely exhaust the in-pass retry budget on the new pass.
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        // Fresh state: retry_count is 0, phase is Inactive.
        assert_eq!(state.recovery_retry_count(), 0);

        // Switch from Inactive to Pending: retry count stays 0.
        state.switch_to_tool_free_recovery();
        assert_eq!(state.recovery_retry_count(), 0);

        // Complete this pass and start a second cycle.
        assert!(state.consume_recovery_pass());
        assert!(state.retry_recovery_pass()); // retry_count becomes 1
        assert_eq!(state.recovery_retry_count(), 1);

        // Switch from Completed to Pending: retry count is not reset
        // (only Inactive triggers the reset).
        assert!(state.consume_recovery_pass());
        assert!(state.finish_recovery_pass());
        state.switch_to_tool_free_recovery();
        assert_eq!(state.recovery_retry_count(), 1, "retry count must NOT be reset when switching from Completed");

        // Consume and retry: the budget should tick up to 2, not start over.
        assert!(state.consume_recovery_pass());
        assert!(state.retry_recovery_pass());
        assert_eq!(state.recovery_retry_count(), 2);
    }

    #[test]
    fn harness_state_tracks_post_tool_recovery_cycles() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        assert_eq!(state.post_tool_recovery_cycles(), 0);
        assert_eq!(state.increment_post_tool_recovery_cycle(), 1);
        assert_eq!(state.post_tool_recovery_cycles(), 1);
        assert_eq!(state.increment_post_tool_recovery_cycle(), 2);
        assert_eq!(state.post_tool_recovery_cycles(), 2);
        assert_eq!(state.increment_post_tool_recovery_cycle(), 3);
        assert_eq!(state.post_tool_recovery_cycles(), 3);
    }

    #[test]
    fn harness_state_tracks_task_tracker_create_signatures() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        assert!(state.record_task_tracker_create_signature(
            "task_tracker::create::{\"title\":\"A\",\"items\":[\"x\"]}".to_string()
        ));
        assert!(!state.record_task_tracker_create_signature(
            "task_tracker::create::{\"title\":\"A\",\"items\":[\"x\"]}".to_string()
        ));
        assert!(state.record_task_tracker_create_signature(
            "task_tracker::create::{\"title\":\"A\",\"items\":[\"y\"]}".to_string()
        ));
    }

    #[test]
    fn harness_state_tracks_successful_readonly_signatures() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        assert!(!state.has_successful_readonly_signature("file_operation:ro:len10-fnv1234"));
        assert!(state.record_successful_readonly_signature("file_operation:ro:len10-fnv1234".to_string()));
        assert!(state.has_successful_readonly_signature("file_operation:ro:len10-fnv1234"));
        assert!(!state.record_successful_readonly_signature("file_operation:ro:len10-fnv1234".to_string()));
    }

    #[test]
    fn harness_state_tracks_identical_shell_command_streak() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        assert_eq!(state.record_shell_command_run("exec_command::cargo check".to_string()), 1);
        assert_eq!(state.record_shell_command_run("exec_command::cargo check".to_string()), 2);
        assert_eq!(state.record_shell_command_run("exec_command::cargo test".to_string()), 1);
        assert_eq!(state.last_shell_command_signature.as_deref(), Some("exec_command::cargo test"));
    }

    #[test]
    fn harness_state_resets_shell_command_streak() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        state.record_shell_command_run("exec_command::cargo check".to_string());
        state.reset_shell_command_run_streak();
        assert_eq!(state.consecutive_same_shell_command_runs, 0);
        assert!(state.last_shell_command_signature.is_none());
    }

    #[test]
    fn harness_state_tracks_file_read_family_streak() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

        assert_eq!(state.record_file_read_family_call("apply_patch::read::src/lib.rs".to_string()), 1);
        assert_eq!(state.record_file_read_family_call("apply_patch::read::src/lib.rs".to_string()), 2);
        assert_eq!(state.record_file_read_family_call("apply_patch::read::src/main.rs".to_string()), 1);

        state.reset_file_read_family_streak();
        assert_eq!(state.consecutive_same_file_read_family_calls, 0);
    }

    #[test]
    fn file_read_path_counts_track_per_path_regardless_of_slice() {
        let mut state =
            HarnessTurnState::new(TurnRunId("run-path".to_string()), TurnId("turn-path".to_string()), 20, 600, 3);

        // Same path, different offsets — each increments the path counter.
        assert_eq!(state.record_file_read_path_call("src/lib.rs".to_string()), 1);
        assert_eq!(state.record_file_read_path_call("src/lib.rs".to_string()), 2);
        assert_eq!(state.record_file_read_path_call("src/lib.rs".to_string()), 3);
        // Different path gets its own counter.
        assert_eq!(state.record_file_read_path_call("src/main.rs".to_string()), 1);
        // Original path continues counting.
        assert_eq!(state.record_file_read_path_call("src/lib.rs".to_string()), 4);

        state.reset_file_read_path_counts();
        assert_eq!(state.record_file_read_path_call("src/lib.rs".to_string()), 1);
    }

    #[test]
    fn harness_state_builds_execution_snapshot() {
        let mut state = HarnessTurnState::new(TurnRunId("run-9".to_string()), TurnId("turn-3".to_string()), 6, 120, 2);
        state.set_phase(TurnPhase::ExecutingTools);

        let snapshot = state.execution_snapshot();
        assert_eq!(snapshot.run_id, "run-9");
        assert_eq!(snapshot.turn_id, "turn-3");
        assert_eq!(snapshot.phase, TurnExecutionPhase::ExecutingTools);
        assert_eq!(snapshot.max_tool_calls, 6);
        assert_eq!(snapshot.max_tool_wall_clock_secs, 120);
        assert_eq!(snapshot.max_tool_retries, 2);
    }

    #[test]
    fn turn_diagnostic_counters_saturate_and_count_recovery_once() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-diagnostics".to_string()),
            TurnId("turn-diagnostics".to_string()),
            4,
            120,
            2,
        );
        state.requested_tool_calls = u32::MAX - 1;
        state.record_requested_tool_calls(usize::MAX);
        state.raw_spooled_bytes = u64::MAX - 2;
        state.model_visible_output_bytes = u64::MAX - 2;
        state.record_tool_output_metrics(true, true, 10, usize::MAX);
        state.activate_recovery("adaptive planning synthesis");
        state.activate_recovery("duplicate activation");

        let diagnostics = state.snapshot_turn_diagnostics(Default::default(), u32::MAX);
        assert_eq!(diagnostics.requested_tool_calls, u32::MAX);
        assert_eq!(diagnostics.reused_results, 1);
        assert_eq!(diagnostics.spooled_results, 1);
        assert_eq!(diagnostics.raw_spooled_bytes, u64::MAX);
        assert_eq!(diagnostics.model_visible_output_bytes, u64::MAX);
        assert_eq!(diagnostics.low_signal_tool_calls, u32::MAX);
        assert_eq!(diagnostics.recovery_activations, 1);
    }

    // --- CrossTurnTracker tests ---

    #[test]
    fn cross_turn_tracker_no_warning_on_first_turn() {
        let mut tracker = CrossTurnTracker::new();
        let read_sigs = vec!["apply_patch::read::src/main.rs".to_string()];
        let written = HashSet::new();
        assert!(tracker.seal_turn(&read_sigs, &written, None, false).is_none());
    }

    #[test]
    fn cross_turn_tracker_detects_repeated_turn() {
        let mut tracker = CrossTurnTracker::new();
        let read_sigs = vec!["apply_patch::read::src/main.rs".to_string()];
        let written = HashSet::new();

        // First turn: no warning
        assert!(tracker.seal_turn(&read_sigs, &written, None, false).is_none());

        // Second turn with same signatures: cross-turn loop detected
        let warning = tracker.seal_turn(&read_sigs, &written, None, false);
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("Cross-turn loop detected"));
    }

    #[test]
    fn cross_turn_tracker_no_false_positive_different_turns() {
        let mut tracker = CrossTurnTracker::new();
        let read_sigs_1 = vec!["apply_patch::read::src/main.rs".to_string()];
        let read_sigs_2 = vec!["apply_patch::read::src/lib.rs".to_string()];
        let written = HashSet::new();

        assert!(tracker.seal_turn(&read_sigs_1, &written, None, false).is_none());
        assert!(tracker.seal_turn(&read_sigs_2, &written, None, false).is_none());
    }

    #[test]
    fn cross_turn_tracker_stuck_no_progress() {
        let mut tracker = CrossTurnTracker::new();
        let written = HashSet::new();

        // Use different signatures each turn to avoid cross-turn loop detection
        // and isolate the stuck (zero-mutation) detection.
        let sigs_1 = vec!["apply_patch::read::src/a.rs".to_string()];
        let sigs_2 = vec!["apply_patch::read::src/b.rs".to_string()];
        let sigs_3 = vec!["apply_patch::read::src/c.rs".to_string()];

        assert!(tracker.seal_turn(&sigs_1, &written, None, false).is_none());
        assert!(tracker.seal_turn(&sigs_2, &written, None, false).is_none());

        // Third consecutive read-only turn: stuck warning
        let warning = tracker.seal_turn(&sigs_3, &written, None, false);
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("No progress detected"));
    }

    #[test]
    fn cross_turn_tracker_mutation_resets_stuck_counter() {
        let mut tracker = CrossTurnTracker::new();
        let empty_written = HashSet::new();

        // Two read-only turns with different signatures (avoid cross-turn loop)
        let sigs_a = vec!["apply_patch::read::src/a.rs".to_string()];
        let sigs_b = vec!["apply_patch::read::src/b.rs".to_string()];
        assert!(tracker.seal_turn(&sigs_a, &empty_written, None, false).is_none());
        assert!(tracker.seal_turn(&sigs_b, &empty_written, None, false).is_none());

        // A mutating turn resets the counter
        let mut written = HashSet::new();
        written.insert("src/main.rs".to_string());
        let sigs_c = vec!["apply_patch::read::src/c.rs".to_string()];
        assert!(tracker.seal_turn(&sigs_c, &written, None, false).is_none());

        // Two more read-only turns: no stuck warning (counter was reset)
        let sigs_d = vec!["apply_patch::read::src/d.rs".to_string()];
        let sigs_e = vec!["apply_patch::read::src/e.rs".to_string()];
        assert!(tracker.seal_turn(&sigs_d, &empty_written, None, false).is_none());
        assert!(tracker.seal_turn(&sigs_e, &empty_written, None, false).is_none());
    }

    #[test]
    fn cross_turn_tracker_empty_turn_no_warning() {
        let mut tracker = CrossTurnTracker::new();
        let empty_sigs: Vec<String> = Vec::new();
        let empty_written = HashSet::new();

        // Empty turns should not trigger warnings or corrupt state
        assert!(tracker.seal_turn(&empty_sigs, &empty_written, None, false).is_none());
        assert!(tracker.seal_turn(&empty_sigs, &empty_written, None, false).is_none());
    }

    #[test]
    fn cross_turn_tracker_order_independent_fingerprint() {
        let mut tracker = CrossTurnTracker::new();
        let written = HashSet::new();

        // Same signatures in different order should produce same fingerprint
        let sigs_a = vec![
            "apply_patch::read::src/main.rs".to_string(),
            "code_search::grep::fn".to_string(),
        ];
        let sigs_b = vec![
            "code_search::grep::fn".to_string(),
            "apply_patch::read::src/main.rs".to_string(),
        ];

        assert!(tracker.seal_turn(&sigs_a, &written, None, false).is_none());
        let warning = tracker.seal_turn(&sigs_b, &written, None, false);
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("Cross-turn loop detected"));
    }
}
