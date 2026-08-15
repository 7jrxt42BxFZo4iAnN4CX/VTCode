//! Guard for blocked tool calls.
//!
//! Tracks consecutive and total blocked tool calls per turn. When the cap is
//! reached, the turn is stopped to prevent retry churn.
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
                "Blocked tool calls reached the recovery-mode cap ({cap}) for this turn. Last blocked call: '{display_tool}'. Stopping turn."
            ),
            format!("Blocked tool calls exceeded the recovery-mode cap ({cap}) for this turn."),
            "blocked_total",
        ),
        (BlockedToolCallFuseTrip::Total { cap }, false) => (
            format!(
                "Blocked tool calls reached per-turn cap ({cap}). Last blocked call: '{display_tool}'. Stopping turn to prevent retry churn."
            ),
            format!("Blocked tool calls exceeded cap ({cap}) for this turn."),
            "blocked_total",
        ),
        (BlockedToolCallFuseTrip::Consecutive { cap }, _) => (
            format!(
                "Consecutive blocked tool calls reached per-turn cap ({cap}). Last blocked call: '{display_tool}'. Stopping turn to prevent retry churn."
            ),
            format!("Consecutive blocked tool calls exceeded cap ({cap}) for this turn."),
            "blocked_streak",
        ),
    };
    let error_content = build_failure_error_content(error_msg, error_label);
    (block_reason, error_content)
}

/// Enforce the blocked tool call guard.
///
/// Returns `Some(TurnHandlerOutcome)` when the guard trips (turn should stop),
/// or `None` when the guard passes (continue processing).
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
    let (block_reason, error_content) = blocked_tool_call_messages(fuse_trip, ctx.is_recovery_active(), &display_tool);
    push_guard_failure_messages(ctx, tool_call_id, tool_name, error_content, &block_reason);

    Some(TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(block_reason) }))
}
