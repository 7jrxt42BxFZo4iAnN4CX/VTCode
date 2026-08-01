use std::collections::HashMap;
use vtcode_core::config::constants::tools;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_ui::tui::app::{InlineHandle, InlineMessageKind, PlanContent};

fn render_created_task_tracker(handle: &InlineHandle, output: &serde_json::Value) {
    let lines = crate::agent::runloop::tool_output::tracker_view_lines(output);
    if lines.is_empty() {
        return;
    }

    // Approval creates the tracker outside the normal tool pipeline, so make
    // the same panel/transcript updates that a regular task_tracker call gets.
    // Without this, the checklist exists on disk but remains invisible until
    // the user manually opens the task panel.
    handle.update_task_panel(lines.clone());
    handle.show_task_panel();
    handle.append_pasted_message(InlineMessageKind::Tool, lines.join("\n"), lines.len());
}

fn markdown_task_description(line: &str) -> Option<(&str, bool)> {
    let trimmed = line.trim().strip_prefix("- ").unwrap_or(line.trim());
    if let Some(description) = trimmed.strip_prefix("[ ] ") {
        return Some((description.trim(), false));
    }
    if let Some(description) = trimmed.strip_prefix("[x] ").or_else(|| trimmed.strip_prefix("[X] ")) {
        return Some((description.trim(), true));
    }

    let (prefix, description) = trimmed.split_once('.')?;
    if prefix.chars().all(|character| character.is_ascii_digit()) {
        let description = description.trim();
        (!description.is_empty()).then_some((description, false))
    } else {
        None
    }
}

fn normalized_task_description(description: &str) -> String {
    description
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn append_unique_task_item(
    items: &mut Vec<serde_json::Value>,
    index_by_description: &mut HashMap<String, usize>,
    item: serde_json::Value,
) {
    let Some(description) = item.get("description").and_then(|value| value.as_str()) else {
        return;
    };
    let key = normalized_task_description(description);
    if key.is_empty() {
        return;
    }

    if let Some(index) = index_by_description.get(&key).copied() {
        let existing = &mut items[index];
        if item["status"].as_str() == Some("completed") && existing["status"].as_str() != Some("completed") {
            existing["status"] = serde_json::Value::String("completed".to_string());
        }
        let existing_files_empty = existing
            .get("files")
            .and_then(serde_json::Value::as_array)
            .is_none_or(Vec::is_empty);
        if existing_files_empty
            && let Some(files) = item.get("files").filter(|files| !files.as_array().is_none_or(Vec::is_empty))
        {
            existing["files"] = files.clone();
        }
        return;
    }

    index_by_description.insert(key, items.len());
    items.push(item);
}

fn task_items_from_plan(plan: &PlanContent) -> Vec<serde_json::Value> {
    let mut items = Vec::new();
    let mut index_by_description = HashMap::new();
    for phase in &plan.phases {
        for step in &phase.steps {
            if step.description.trim().is_empty() {
                continue;
            }
            let mut item = serde_json::json!({
                "description": step.description.trim(),
                "status": if step.completed { "completed" } else { "pending" },
            });
            if !step.files.is_empty()
                && let Ok(files) = serde_json::to_value(&step.files)
            {
                item["files"] = files;
            }
            append_unique_task_item(&mut items, &mut index_by_description, item);
        }
    }

    // Sparse plans may omit a phase heading. Preserve their numbered and
    // checkbox steps so approval still produces a usable tracker.
    if items.is_empty() {
        for line in plan.raw_content.lines() {
            let Some((description, completed)) = markdown_task_description(line) else {
                continue;
            };
            if description.is_empty() {
                continue;
            }
            append_unique_task_item(
                &mut items,
                &mut index_by_description,
                serde_json::json!({
                    "description": description,
                    "status": if completed { "completed" } else { "pending" },
                }),
            );
        }
    }

    items
}

/// Create the implementation checklist while the planning gate is still
/// active. `PlanningTaskTrackerTool` deliberately rejects calls after
/// `finish_planning_workflow`, so this is the single lifecycle boundary for
/// approved-plan task handoff.
pub(crate) async fn create_task_tracker_from_active_plan(
    tool_registry: &ToolRegistry,
    handle: &InlineHandle,
) -> anyhow::Result<()> {
    let plan_state = tool_registry.planning_workflow_state();
    let plan_file = match plan_state.get_plan_file().await {
        Some(path) => path,
        None => return Ok(()),
    };

    let plan_content = match tokio::fs::read_to_string(&plan_file).await {
        Ok(content) => content,
        Err(_) => return Ok(()),
    };

    if plan_content.trim().is_empty() {
        return Ok(());
    }

    let plan = PlanContent::from_markdown(
        plan_file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Implementation Plan")
            .to_string(),
        &plan_content,
        plan_file.to_str().map(|s| s.to_string()),
    );
    let items = task_items_from_plan(&plan);
    if items.is_empty() {
        return Ok(());
    }

    let tool = match tool_registry.get_tool(tools::TASK_TRACKER) {
        Some(tool) => tool,
        None => return Ok(()),
    };
    let args = serde_json::json!({
        "action": "create",
        "title": plan.title,
        "items": items,
    });

    match tool.execute(args).await {
        Ok(result) => {
            render_created_task_tracker(handle, &result);
            let message = result
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("Task tracker created");
            tracing::info!(message = %message, "Task tracker created during approved-plan handoff");
        }
        Err(err) => {
            tracing::warn!(error = %err, "Failed to auto-create task tracker from approved plan");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{PlanContent, task_items_from_plan};

    #[test]
    fn sparse_approved_plan_is_distilled_into_tracker_items() {
        let plan = PlanContent::from_markdown(
            "Launch plan".to_string(),
            "Summary\nImprove startup latency.\n\n1. Measure startup\n2. Defer eager setup\n- [x] Verify with cargo check",
            None,
        );

        let items = task_items_from_plan(&plan);

        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["description"], "Measure startup");
        assert_eq!(items[0]["status"], "pending");
        assert_eq!(items[2]["description"], "Verify with cargo check");
        assert_eq!(items[2]["status"], "completed");
    }

    #[test]
    fn repeated_plan_steps_are_deduplicated_in_order() {
        let plan = PlanContent::from_markdown(
            "Launch plan".to_string(),
            "Summary\nImprove the runtime.\n\n## Phase 1\n1. Inspect the runtime\n2. Apply the fix\n\n## Phase 2\n1. inspect   the runtime\n[x] Verify the fix\n[x] APPLY THE FIX",
            None,
        );

        let items = task_items_from_plan(&plan);

        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["description"], "Inspect the runtime");
        assert_eq!(items[1]["description"], "Apply the fix");
        assert_eq!(items[1]["status"], "completed");
        assert_eq!(items[2]["description"], "Verify the fix");
    }
}
