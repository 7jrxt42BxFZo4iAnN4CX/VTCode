//! Privacy-preserving summaries for JSONL agent traces.

use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;

/// Aggregate token and prompt-cache usage found in a trace.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub struct TokenUsage {
    /// Total prompt/input tokens.
    pub input_tokens: u64,
    /// Total generated/output tokens.
    pub output_tokens: u64,
    /// Total prompt tokens served from cache.
    pub cached_input_tokens: u64,
    /// Total tokens used to create cache entries.
    pub cache_creation_tokens: u64,
}

/// Statistics over recorded latency samples, in milliseconds.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq)]
pub struct LatencyStatistics {
    /// Number of latency samples.
    pub count: u64,
    /// Sum of latency samples.
    pub total_ms: u64,
    /// Arithmetic mean, or `None` when no samples were recorded.
    pub mean_ms: Option<f64>,
    /// Median sample, or `None` when no samples were recorded.
    pub p50_ms: Option<u64>,
    /// 95th percentile sample, or `None` when no samples were recorded.
    pub p95_ms: Option<u64>,
    /// Largest recorded sample, or `None` when no samples were recorded.
    pub max_ms: Option<u64>,
}

/// Redacted aggregate facts extracted from DeepSeek or VT Code JSONL traces.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct HarnessTraceSummary {
    /// Number of execution turns.
    pub turns: u64,
    /// Number of agent steps.
    pub steps: u64,
    /// Number of tool calls.
    pub tool_calls: u64,
    /// Tool name to invocation count.
    pub tool_counts: BTreeMap<String, u64>,
    /// Canonical error category to count.
    pub error_categories: BTreeMap<String, u64>,
    /// Latency aggregate for all recognized samples.
    pub latency: LatencyStatistics,
    /// Total UTF-8 byte length of tool outputs, without retaining output text.
    pub output_bytes: u64,
    /// Number of calls after the first call for each tool name.
    pub repeated_calls: u64,
    /// Repeated calls grouped by tool name.
    pub repeated_tool_counts: BTreeMap<String, u64>,
    /// Aggregate model token usage.
    pub token_usage: TokenUsage,
    /// Lines that were not valid JSON objects.
    pub malformed_lines: u64,
    /// Valid JSON objects with no recognized trace shape.
    pub unrecognized_lines: u64,
}

/// Analyze JSONL text while retaining only aggregate, non-sensitive facts.
pub fn analyze_jsonl(input: &str) -> Result<HarnessTraceSummary> {
    let mut summary = HarnessTraceSummary::default();
    let mut latencies = Vec::new();

    for line in input.lines().filter(|line| !line.trim().is_empty()) {
        let value = match serde_json::from_str::<Value>(line) {
            Ok(value) => value,
            Err(_) => {
                summary.malformed_lines = summary.malformed_lines.saturating_add(1);
                continue;
            }
        };

        if !record_value(&value, &mut summary, &mut latencies) {
            summary.unrecognized_lines = summary.unrecognized_lines.saturating_add(1);
        }
    }

    summary.latency = latency_statistics(&mut latencies);
    Ok(summary)
}

/// Analyze a JSONL trace file and add path context to filesystem errors.
pub fn analyze_jsonl_file(path: impl AsRef<Path>) -> Result<HarnessTraceSummary> {
    let path = path.as_ref();
    let input = fs::read_to_string(path).with_context(|| format!("read trace file {}", path.display()))?;
    analyze_jsonl(&input).with_context(|| format!("analyze trace file {}", path.display()))
}

fn record_value(value: &Value, summary: &mut HarnessTraceSummary, latencies: &mut Vec<u64>) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let event_type = string_field(object, &["type", "event", "kind"]);
    let is_thread_event = event_type.is_some_and(|event| {
        event.starts_with("thread.")
            || event.starts_with("turn.")
            || event.starts_with("item.")
            || matches!(event, "error" | "context.reset" | "permission.requested" | "permission.resolved")
    });

    let mut recognized = is_thread_event;
    if matches!(event_type, Some("turn.started")) {
        summary.turns = summary.turns.saturating_add(1);
    }
    if matches!(event_type, Some("error" | "turn.failed")) {
        add_error(summary, string_field(object, &["error_category", "category", "code"]).unwrap_or("error"));
        recognized = true;
    }

    if let Some(item) = object.get("item").and_then(Value::as_object) {
        if matches!(event_type, Some("item.completed")) {
            recognized |= record_item(item, summary);
            if let Some(bytes) = output_bytes(item) {
                summary.output_bytes = summary.output_bytes.saturating_add(bytes);
            }
        } else {
            recognized = true;
        }
    }

    let deepseek_tool = string_field(object, &["tool", "tool_name", "name"]).or_else(|| {
        object
            .get("function")
            .and_then(Value::as_object)
            .and_then(|f| string_field(f, &["name"]))
    });
    let has_step = object.contains_key("step") || object.contains_key("step_id");
    if deepseek_tool.is_some() || has_step {
        recognized = true;
        if has_step {
            summary.steps = summary.steps.saturating_add(1);
        }
        if let Some(tool) = deepseek_tool {
            record_tool(summary, tool);
        }
    }

    if let Some(latency) = number_field(object, &["latency_ms", "duration_ms", "latency"]) {
        latencies.push(latency);
        recognized = true;
    }
    if let Some(bytes) = output_bytes(object) {
        summary.output_bytes = summary.output_bytes.saturating_add(bytes);
        recognized = true;
    }
    if let Some(category) = error_category(object) {
        add_error(summary, &category);
        recognized = true;
    }
    record_usage(object, &mut summary.token_usage);
    recognized || has_usage(object)
}

fn record_item(item: &serde_json::Map<String, Value>, summary: &mut HarnessTraceSummary) -> bool {
    let Some(details_type) = item.get("type").and_then(Value::as_str) else {
        return false;
    };
    match details_type {
        "tool_invocation" | "mcp_tool_call" => {
            summary.steps = summary.steps.saturating_add(1);
            if let Some(tool) = string_field(item, &["tool_name", "name"]) {
                record_tool(summary, tool);
            }
            if string_field(item, &["status", "outcome"])
                .is_some_and(|status| status != "completed" && status != "success")
            {
                add_error(summary, string_field(item, &["outcome", "status"]).unwrap_or("tool_error"));
            }
            true
        }
        "command_execution" => {
            summary.steps = summary.steps.saturating_add(1);
            record_tool(summary, "command_execution");
            if item
                .get("status")
                .and_then(Value::as_str)
                .is_some_and(|status| status == "failed")
            {
                add_error(summary, "command_failed");
            }
            true
        }
        "tool_output" => true,
        "harness" => {
            if let Some(event) = item.get("event").and_then(Value::as_str)
                && event.ends_with("failed")
            {
                add_error(summary, item.get("error_category").and_then(Value::as_str).unwrap_or(event));
            }
            true
        }
        "error" => {
            add_error(summary, "error");
            true
        }
        _ => false,
    }
}

fn record_tool(summary: &mut HarnessTraceSummary, tool: &str) {
    summary.tool_calls = summary.tool_calls.saturating_add(1);
    let count = summary.tool_counts.entry(tool.to_owned()).or_default();
    if *count > 0 {
        summary.repeated_calls = summary.repeated_calls.saturating_add(1);
        *summary.repeated_tool_counts.entry(tool.to_owned()).or_default() += 1;
    }
    *count += 1;
}

fn add_error(summary: &mut HarnessTraceSummary, category: &str) {
    *summary.error_categories.entry(category.to_owned()).or_default() += 1;
}

fn string_field<'a>(object: &'a serde_json::Map<String, Value>, names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|name| object.get(*name).and_then(Value::as_str))
}

fn number_field(object: &serde_json::Map<String, Value>, names: &[&str]) -> Option<u64> {
    names.iter().find_map(|name| object.get(*name).and_then(Value::as_u64))
}

fn output_bytes(object: &serde_json::Map<String, Value>) -> Option<u64> {
    ["output", "aggregated_output", "tool_output", "result"]
        .iter()
        .find_map(|name| object.get(*name).and_then(Value::as_str).map(|output| output.len() as u64))
}

fn error_category(object: &serde_json::Map<String, Value>) -> Option<String> {
    if let Some(category) = string_field(object, &["error_category", "error_code"]) {
        return Some(category.to_owned());
    }
    match object.get("error") {
        Some(Value::String(error)) => Some(canonical_error_category(error).to_owned()),
        Some(Value::Object(error)) => string_field(error, &["category", "code", "type"]).map(str::to_owned),
        _ => None,
    }
}

fn canonical_error_category(error: &str) -> &'static str {
    let error = error.to_ascii_lowercase();
    if error.contains("timeout") {
        "timeout"
    } else if error.contains("permission") || error.contains("denied") {
        "permission_denied"
    } else if error.contains("network") || error.contains("connection") {
        "network"
    } else if error.contains("parse") || error.contains("json") {
        "parse"
    } else {
        "error"
    }
}

fn usage_value(object: &serde_json::Map<String, Value>) -> Option<&serde_json::Map<String, Value>> {
    ["usage", "tokens"]
        .iter()
        .find_map(|name| object.get(*name).and_then(Value::as_object))
}

fn has_usage(object: &serde_json::Map<String, Value>) -> bool {
    usage_value(object).is_some() || object.keys().any(|key| key.ends_with("_tokens"))
}

fn record_usage(object: &serde_json::Map<String, Value>, usage: &mut TokenUsage) {
    let source = usage_value(object).unwrap_or(object);
    usage.input_tokens = usage
        .input_tokens
        .saturating_add(number_field(source, &["input", "input_tokens", "prompt_tokens"]).unwrap_or(0));
    usage.output_tokens = usage
        .output_tokens
        .saturating_add(number_field(source, &["output", "output_tokens", "completion_tokens"]).unwrap_or(0));
    usage.cached_input_tokens = usage
        .cached_input_tokens
        .saturating_add(number_field(source, &["cached", "cached_tokens", "cached_input_tokens"]).unwrap_or(0));
    usage.cache_creation_tokens = usage
        .cache_creation_tokens
        .saturating_add(number_field(source, &["cache_creation", "cache_creation_tokens"]).unwrap_or(0));
}

fn latency_statistics(samples: &mut [u64]) -> LatencyStatistics {
    if samples.is_empty() {
        return LatencyStatistics::default();
    }
    samples.sort_unstable();
    let total_ms = samples.iter().copied().sum();
    let percentile = |percent: usize| samples[((samples.len() - 1) * percent).div_ceil(100)];
    LatencyStatistics {
        count: samples.len() as u64,
        total_ms,
        mean_ms: Some(total_ms as f64 / samples.len() as f64),
        p50_ms: Some(percentile(50)),
        p95_ms: Some(percentile(95)),
        max_ms: samples.last().copied(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THREAD_EVENT_TRACE: &str = r#"{"type":"turn.started"}
{"type":"item.completed","item":{"id":"1","type":"tool_invocation","tool_name":"exec_command","status":"completed"}}
{"type":"item.completed","item":{"id":"2","type":"tool_invocation","tool_name":"exec_command","status":"failed","outcome":"timeout"}}
{"type":"turn.completed","usage":{"input_tokens":8,"cached_input_tokens":3,"cache_creation_tokens":1,"output_tokens":5}}
{"type":"item.completed","item":{"id":"3","type":"tool_output","output":"private output"}}
"#;

    #[test]
    fn summarizes_deepseek_baseline_without_retaining_raw_text() {
        let trace = r#"
{"step":1,"tool":"exec_command","latency_ms":12,"output":"secret command output","tokens":{"input":100,"output":20,"cached":40}}
{"step":2,"tool":"read_file","latency_ms":20,"error":"timeout"}
"#;

        let summary = analyze_jsonl(trace).expect("trace should parse");

        assert_eq!(summary.steps, 2);
        assert_eq!(summary.tool_calls, 2);
        assert_eq!(summary.tool_counts["exec_command"], 1);
        assert_eq!(summary.tool_counts["read_file"], 1);
        assert_eq!(summary.error_categories["timeout"], 1);
        assert_eq!(summary.output_bytes, 21);
        assert_eq!(summary.token_usage.input_tokens, 100);
        assert_eq!(summary.token_usage.output_tokens, 20);
        assert_eq!(summary.token_usage.cached_input_tokens, 40);
        assert!(
            !serde_json::to_string(&summary)
                .expect("summary should serialize")
                .contains("secret command output")
        );
    }

    #[test]
    fn matches_known_deepseek_baseline_counts_with_compact_fixture() {
        let mut trace = String::new();
        for step in 1..=453 {
            trace.push_str(&format!(
                r#"{{"step":{step},"tool":"exec_command"}}
"#
            ));
        }
        trace.push_str(
            r#"{"tool":"exec_command"}
{"tool":"read_file"}
{"tool":"read_file"}
{"tool":"write_file"}
{"tool":"write_file"}
{"tool":"search"}
{"tool":"search"}
{"tool":"search"}
{"tool":"search"}
{"tool":"search"}
{"tool":"search"}
{"tool":"search"}
{"tool":"search"}
{"tool":"search"}
{"tool":"search"}
{"error":"timeout"}
{"error":"timeout"}
{"error":"timeout"}
{"error":"timeout"}
{"error":"timeout"}
{"error":"timeout"}
{"error":"timeout"}
{"error":"timeout"}
{"error":"timeout"}
{"error":"timeout"}
{"error":"timeout"}
{"error":"timeout"}
{"error":"timeout"}
{"error":"timeout"}
{"error":"timeout"}
{"error":"timeout"}
{"error":"timeout"}
{"error":"timeout"}
{"error":"timeout"}
{"error":"timeout"}
"#,
        );

        let summary = analyze_jsonl(&trace).expect("baseline fixture should parse");

        assert_eq!(summary.steps, 453);
        assert_eq!(summary.tool_calls, 468);
        assert_eq!(summary.error_categories.values().sum::<u64>(), 20);
    }

    #[test]
    fn parses_thread_events_and_skips_bad_or_unknown_lines() {
        let input = format!("{}\nnot json\n{{\"future\":true}}\n", THREAD_EVENT_TRACE);
        let summary = analyze_jsonl(&input).expect("trace should parse");

        assert_eq!(summary.turns, 1);
        assert_eq!(summary.steps, 2);
        assert_eq!(summary.tool_calls, 2);
        assert_eq!(summary.repeated_calls, 1);
        assert_eq!(summary.error_categories["timeout"], 1);
        assert_eq!(summary.output_bytes, 14);
        assert_eq!(summary.token_usage.input_tokens, 8);
        assert_eq!(summary.token_usage.cached_input_tokens, 3);
        assert_eq!(summary.malformed_lines, 1);
        assert_eq!(summary.unrecognized_lines, 1);
    }

    #[test]
    fn reports_latency_statistics_and_file_errors_with_context() {
        let summary = analyze_jsonl(
            "{\"step\":1,\"latency_ms\":30}\n{\"step\":2,\"latency_ms\":10}\n{\"step\":3,\"latency_ms\":20}\n",
        )
        .expect("latency trace should parse");
        assert_eq!(summary.latency.count, 3);
        assert_eq!(summary.latency.total_ms, 60);
        assert_eq!(summary.latency.p50_ms, Some(20));
        assert_eq!(summary.latency.p95_ms, Some(30));

        let missing = analyze_jsonl_file("/path/that/does/not/exist.jsonl").expect_err("missing file should fail");
        assert!(missing.to_string().contains("read trace file"));
    }
}
