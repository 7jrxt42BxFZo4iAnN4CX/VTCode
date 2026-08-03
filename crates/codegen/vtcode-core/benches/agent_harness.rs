#![allow(missing_docs, clippy::expect_used)]

use std::hint::black_box;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use tokio::runtime::Runtime;
use tokio::sync::RwLock;
use vtcode_core::core::agent::harness_kernel::{
    HarnessRequestPlanInput, PreparedToolBatch, PreparedToolCall, build_harness_request_plan,
};
use vtcode_core::llm::provider::{Message, ToolChoice, ToolDefinition};
use vtcode_core::prompts::{FewShotExample, FewShotStore, resolve_system_prompt_layers, sort_tool_definitions};
use vtcode_core::tools::registry::SessionToolCatalogState;
use vtcode_indexer::file_search::{FileIndexCache, FileSearchConfig, run_with_index};

fn sample_tool(name: &str) -> ToolDefinition {
    ToolDefinition::function(
        name.to_string(),
        format!("Tool {name}"),
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            }
        }),
    )
}

fn sample_tools(count: usize) -> Arc<Vec<ToolDefinition>> {
    Arc::new((0..count).map(|index| sample_tool(&format!("tool_{index}"))).collect())
}

fn sample_messages(count: usize) -> Vec<Message> {
    (0..count).map(|index| Message::user(format!("message {index}"))).collect()
}

fn request_plan_benchmark(c: &mut Criterion) {
    let tools = sample_tools(24);
    let messages = sample_messages(32);

    c.bench_function("agent_harness_request_plan_with_tools", |b| {
        b.iter(|| {
            black_box(build_harness_request_plan(HarnessRequestPlanInput {
                messages: Arc::new(messages.clone()),
                system_prompt: "System prompt\n[Runtime Context]\nturn=12".to_string(),
                tools: Some(Arc::clone(&tools)),
                model: "gpt-5".to_string(),
                max_tokens: Some(2000),
                temperature: Some(0.7),
                stream: true,
                tool_choice: Some(ToolChoice::auto()),
                parallel_tool_config: None,
                reasoning_effort: None,
                verbosity: None,
                metadata: None,
                context_management: None,
                previous_response_id: Some("resp_123".to_string()),
                prompt_cache_key: Some("session:test".to_string()),
                prompt_cache_profile: None,
                tool_catalog_hash: None,
                system_prompt_prefix_hash: None,
            }))
        })
    });
}

fn prepared_batch_planning_benchmark(c: &mut Criterion) {
    let calls: Vec<PreparedToolCall> = (0..48)
        .map(|index| {
            let readonly = index % 5 != 0;
            PreparedToolCall::new(
                format!("tool_{index}"),
                readonly,
                readonly,
                serde_json::json!({ "path": format!("src/file_{index}.rs") }),
            )
        })
        .collect();

    c.bench_function("agent_harness_prepared_batch_plan", |b| {
        b.iter(|| black_box(PreparedToolBatch::plan(calls.clone(), true)))
    });
}

fn tool_catalog_projection_benchmark(c: &mut Criterion) {
    let runtime = Runtime::new().expect("criterion tokio runtime");
    let state = Arc::new(SessionToolCatalogState::new());
    let tools = Arc::new(RwLock::new((*sample_tools(32)).clone()));

    runtime.block_on(async {
        let _ = state.filtered_snapshot_with_stats(&tools, true, false).await;
    });

    c.bench_function("agent_harness_tool_catalog_cache_hit", |b| {
        b.iter(|| {
            let state = Arc::clone(&state);
            let tools = Arc::clone(&tools);
            runtime.block_on(async move { black_box(state.filtered_snapshot_with_stats(&tools, true, false).await) })
        })
    });

    c.bench_function("agent_harness_tool_catalog_cache_miss", |b| {
        b.iter(|| {
            let state = Arc::clone(&state);
            let tools = Arc::clone(&tools);
            runtime.block_on(async move {
                state.note_explicit_refresh("benchmark");
                black_box(state.filtered_snapshot_with_stats(&tools, true, false).await)
            })
        })
    });
}

fn prompt_resource_cache_hit_benchmark(c: &mut Criterion) {
    let workspace = tempfile::tempdir().expect("benchmark workspace");
    let examples_dir = workspace.path().join(".vtcode/prompts/examples");
    std::fs::create_dir_all(&examples_dir).expect("benchmark examples directory");
    std::fs::write(
        examples_dir.join("benchmark.md"),
        "---\ntags: [benchmark, cache]\n---\nA cached prompt resource.\n",
    )
    .expect("benchmark example");

    let first = FewShotStore::load(Some(workspace.path()), None);
    assert_eq!(first.len(), 1);
    c.bench_function("prompt_resource_cache_hit_few_shot", |b| {
        b.iter(|| black_box(FewShotStore::load(Some(workspace.path()), None)))
    });

    let runtime = Runtime::new().expect("criterion tokio runtime");
    runtime.block_on(resolve_system_prompt_layers(workspace.path()));
    c.bench_function("prompt_resource_cache_hit_system_layers", |b| {
        b.iter(|| runtime.block_on(async { black_box(resolve_system_prompt_layers(workspace.path()).await) }))
    });
}

fn few_shot_selection_benchmark(c: &mut Criterion) {
    let examples = (0..128)
        .map(|index| FewShotExample {
            id: format!("example-{index:03}"),
            tags: vec!["search".to_string(), format!("topic-{index}")],
            summary: "benchmark example".to_string(),
            body: "search and inspect a source file before editing it".to_string(),
            token_count: 16,
            source_path: std::path::PathBuf::from(format!("/tmp/example-{index:03}.md")),
        })
        .collect();
    let store = FewShotStore::from_examples(examples);

    c.bench_function("few_shot_selection_normalized_query", |b| {
        b.iter(|| black_box(store.select("please search topic-37 before editing", 800)))
    });
}

fn tool_definition_sorting_benchmark(c: &mut Criterion) {
    let tools = (0..96)
        .rev()
        .map(|index| sample_tool(&format!("catalog_tool_{index:03}")))
        .collect::<Vec<_>>();

    c.bench_function("tool_definition_sorting_catalog_refresh", |b| {
        b.iter(|| black_box(sort_tool_definitions(tools.clone())))
    });
}

fn indexed_file_search_scoring_benchmark(c: &mut Criterion) {
    let workspace = tempfile::tempdir().expect("benchmark workspace");
    for index in 0..256 {
        let path = workspace.path().join(format!("src/module_{index:03}/widget_{index:03}.rs"));
        std::fs::create_dir_all(path.parent().expect("benchmark parent")).expect("benchmark directory");
        std::fs::write(path, "fn widget() {}\n").expect("benchmark source");
    }

    let cache = FileIndexCache::new(workspace.path().to_path_buf(), Vec::new(), false, 2);
    let runtime = Runtime::new().expect("criterion tokio runtime");
    let warm_config = indexed_search_config(workspace.path());
    runtime
        .block_on(run_with_index(warm_config, &cache))
        .expect("warm indexed search");

    c.bench_function("indexed_file_search_scoring_cache_hit", |b| {
        b.iter(|| {
            let result = runtime
                .block_on(run_with_index(indexed_search_config(workspace.path()), &cache))
                .expect("indexed search");
            black_box(result.matches.len())
        })
    });
}

fn indexed_search_config(workspace: &std::path::Path) -> FileSearchConfig {
    FileSearchConfig {
        pattern_text: "widget".to_string(),
        limit: NonZero::new(32).expect("non-zero limit"),
        search_directory: workspace.to_path_buf(),
        exclude: Vec::new(),
        threads: NonZero::new(2).expect("non-zero threads"),
        cancel_flag: Arc::new(AtomicBool::new(false)),
        compute_indices: false,
        respect_gitignore: false,
    }
}

criterion_group!(
    benches,
    request_plan_benchmark,
    prepared_batch_planning_benchmark,
    tool_catalog_projection_benchmark,
    prompt_resource_cache_hit_benchmark,
    few_shot_selection_benchmark,
    tool_definition_sorting_benchmark,
    indexed_file_search_scoring_benchmark
);
criterion_main!(benches);
