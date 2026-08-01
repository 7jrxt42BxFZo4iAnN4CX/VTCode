use anyhow::Result;
use std::path::Path;
use vtcode_core::primary_agent::{ActivePrimaryAgent, ActivePrimaryAgentState};
use vtcode_core::tools::registry::ToolRegistry;

use crate::agent::runloop::unified::turn::primary_agent_runtime::{
    builtin_primary_agent_specs, load_primary_agent_specs, resolve_approved_plan_execution_agent,
};

pub(super) const PLAN_APPROVED_EXECUTION_DIRECTIVE: &str = "Execution handoff is active. Any earlier message saying tools are disabled or implementation is paused belongs to the completed planning/recovery turn and is stale. Tools are enabled now. Do not report that work is paused, ask to wait, or request another confirmation. Start implementation immediately: execute the approved plan step by step beginning with the first pending step. Before the first implementation action, use task_tracker with action=list and mark the first pending task in_progress; update each task as work and verification complete. Use cargo nextest run --locked for Rust verification; never emit a raw <tool_call> block as text. Finish with a concise execution summary covering the outcome, changed files, verification performed, and remaining blockers.";
pub(super) const PLAN_APPROVED_EXECUTION_INPUT: &str = "Implement the approved plan now.";

pub(super) async fn apply_primary_agent_tool_policy_overrides(
    tool_registry: &ToolRegistry,
    active_agent: &ActivePrimaryAgent,
) {
    for (tool_name, policy) in &active_agent.tool_policy_overrides {
        if let Err(err) = tool_registry.set_tool_policy(tool_name, policy.clone()).await {
            tracing::warn!(
                tool = %tool_name,
                error = %err,
                "Failed to apply restored primary-agent tool policy"
            );
        }
    }
}

/// Select the write-capable agent that must own an approved-plan execution.
///
/// Approval is a hard runtime boundary. If discovery returns a stale or
/// read-only catalog, falling back to the built-in `build` spec is safer than
/// continuing with the planning agent and letting the first mutation fail
/// under the planning policy.
pub(super) async fn select_approved_plan_execution_agent(
    active_primary_agent: &mut ActivePrimaryAgentState,
    tool_registry: &ToolRegistry,
    workspace: &Path,
    requested_agent: Option<&str>,
    configured_default: Option<&str>,
) -> Result<String> {
    let mut specs = match load_primary_agent_specs(tool_registry, workspace).await {
        Ok(specs) => specs,
        Err(err) => {
            tracing::warn!(error = %err, "Primary-agent discovery failed during approved-plan handoff; using built-ins");
            builtin_primary_agent_specs()
        }
    };
    let mut execution_agent = resolve_approved_plan_execution_agent(requested_agent, configured_default, &specs);
    if execution_agent.is_none() {
        specs = builtin_primary_agent_specs();
        execution_agent = resolve_approved_plan_execution_agent(requested_agent, configured_default, &specs);
    }

    let execution_agent = execution_agent.ok_or_else(|| {
        anyhow::anyhow!(
            "No write-capable primary agent is available to execute the approved plan; the plan was not started"
        )
    })?;
    active_primary_agent
        .select_from_specs(&specs, &execution_agent)
        .map_err(|err| {
            anyhow::anyhow!("Failed to activate approved-plan execution agent '{execution_agent}': {err}")
        })?;
    Ok(execution_agent)
}
