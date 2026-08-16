//! Guard for blocked tool calls.
//!
//! Tracks consecutive and total blocked tool calls per turn. When the cap is
//! reached, the current batch is closed and one bounded tool-free recovery
//! pass is scheduled to prevent retry churn without dropping the turn.
//!
//! The guard has two thresholds:
//! - **Consecutive cap**: Stops the turn after N consecutive blocked calls
//! - **Total fuse**: Stops the turn after M total blocked calls (even if not consecutive)
//!
//! Recovery mode uses a tighter total fuse than normal mode.

use serde_json::Value;
use vtcode_core::config::constants::defaults::DEFAULT_MAX_CONSECUTIVE_BLOCKED_TOOL_CALLS_PER_TURN;
use vtcode_core::tools::registry::labels::tool_action_label;

use super::super::build_failure_error_content;
use super::common::push_guard_failure_messages;
use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext};

/// Get the max consecutive blocked tool calls per turn from config.
pub(crate) fn max_consecutive_blocked_tool_calls_per_turn(ctx: &TurnProcessingContext<'_>) -> usize {
    ctx.vt_cfg
        .map(|cfg| cfg.tools.max_consecutive_blocked_tool_calls_per_turn)
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_CONSECUTIVE_BLOCKED_TOOL_CALLS_PER_TURN)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BlockedToolCallLimits {
    pub(crate) consecutive_cap: usize,
    pub(crate) total_cap: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BlockedToolCallFuseTrip {
    Consecutive { cap: usize },
    Total { cap: usize },
}

impl BlockedToolCallFuseTrip {
    pub(crate) const fn cap(self) -> usize {
        match self {
            Self::Consecutive { cap } | Self::Total { cap } => cap,
        }
    }

    pub(crate) const fn metric(self) -> &'static str {
        match self {
            Self::Consecutive { .. } => "blocked_streak_break",
            Self::Total { .. } => "blocked_total_break",
        }
    }
}

pub(crate) fn blocked_tool_call_limits(ctx: &TurnProcessingContext<'_>) -> BlockedToolCallLimits {
    let consecutive_cap = max_consecutive_blocked_tool_calls_per_turn(ctx);
    let total_cap = if ctx.is_recovery_active() {
        consecutive_cap
    } else if ctx.is_planning_active() {
        consecutive_cap.saturating_mul(4)
    } else {
        consecutive_cap.saturating_mul(2)
    };

    BlockedToolCallLimits { consecutive_cap, total_cap }
}

pub(crate) fn blocked_tool_call_fuse_trip(
    consecutive_blocked_tool_calls: usize,
    blocked_tool_calls: usize,
    limits: BlockedToolCallLimits,
) -> Option<BlockedToolCallFuseTrip> {
    if blocked_tool_calls > limits.total_cap {
        Some(BlockedToolCallFuseTrip::Total { cap: limits.total_cap })
    } else if consecutive_blocked_tool_calls > limits.consecutive_cap {
        Some(BlockedToolCallFuseTrip::Consecutive { cap: limits.consecutive_cap })
    } else {
        None
    }
}

/// Build the block reason and error content for a blocked tool call fuse trip.
pub(crate) fn blocked_tool_call_messages(
    fuse_trip: BlockedToolCallFuseTrip,
    recovery_mode: bool,
    display_tool: &str,
) -> (String, String) {
    let (block_reason, error_msg, error_label) = match (fuse_trip, recovery_mode) {
        (BlockedToolCallFuseTrip::Total { cap }, true) => (
            format!(
                "Recovery tool-call limit reached after {cap} blocked calls. Last blocked call: '{display_tool}'. Tools remain disabled while the recovery response is finalized."
            ),
            format!("The recovery tool-call limit of {cap} blocked calls was exceeded for this turn."),
            "blocked_total",
        ),
        (BlockedToolCallFuseTrip::Total { cap }, false) => (
            format!(
                "Blocked tool-call limit reached after {cap} total blocked calls this turn. Last blocked call: '{display_tool}'. A bounded recovery response will run without more tool calls."
            ),
            format!("The tool-call safety limit of {cap} blocked calls was exceeded for this turn."),
            "blocked_total",
        ),
        (BlockedToolCallFuseTrip::Consecutive { cap }, _) => (
            format!(
                "Blocked tool-call limit reached after {cap} consecutive blocked calls. Last blocked call: '{display_tool}'. A bounded recovery response will run without more tool calls."
            ),
            format!("The tool-call safety limit of {cap} consecutive blocked calls was exceeded for this turn."),
            "blocked_streak",
        ),
    };
    let error_content = build_failure_error_content(error_msg, error_label);
    (block_reason, error_content)
}

/// Enforce the blocked tool call guard.
///
/// Returns `Some(TurnHandlerOutcome)` when the guard trips, or `None` when the
/// guard passes. A fuse trip returns `Continue` once so the caller can flush
/// the current tool responses and schedule bounded recovery.
pub(crate) fn enforce_blocked_tool_call_guard(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    tool_name: &str,
    args: &Value,
) -> Option<TurnHandlerOutcome> {
    let streak = ctx.record_blocked_tool_call();
    let blocked_total = ctx.blocked_tool_calls();
    let limits = blocked_tool_call_limits(ctx);

    if ctx.is_recovery_active() && !ctx.recovery_pass_used() {
        return Some(TurnHandlerOutcome::Continue);
    }

    let fuse_trip = blocked_tool_call_fuse_trip(streak, blocked_total, limits)?;
    let display_tool = tool_action_label(tool_name, args);
    let recovery_active = ctx.is_recovery_active();
    let (block_reason, error_content) = blocked_tool_call_messages(fuse_trip, recovery_active, &display_tool);

    // Clear any inline loading placeholder and restore the input status so
    // the UI doesn't remain in a stale "loading" or shimmer state after the
    // guard trips.
    ctx.reset_input_to_default_placeholder();
    ctx.restore_input_status(None, None);

    if !recovery_active {
        // Keep the current tool response contiguous with any later responses
        // in the assistant batch. The recovery directive is appended only
        // after the caller drains those remaining calls.
        ctx.push_tool_response(tool_call_id, Some(tool_name), error_content);
        ctx.harness_state.arm_blocked_tool_recovery(block_reason);
        Some(TurnHandlerOutcome::Continue)
    } else {
        push_guard_failure_messages(ctx, tool_call_id, tool_name, error_content, &block_reason);
        Some(TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(block_reason) }))
    }
}
