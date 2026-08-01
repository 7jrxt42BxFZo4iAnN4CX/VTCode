use std::path::{Path, PathBuf};

use chrono::Utc;
use vtcode_config::loader::VTCodeConfig;
use vtcode_core::core::agent::features::FeatureSet;
use vtcode_core::exec::events::{ThreadEvent, ThreadStartedEvent};

use crate::agent::runloop::unified::inline_events::harness::{
    HARNESS_LOG_MAX_AGE_DAYS, HarnessEventEmitter, default_harness_log_dir, prune_old_harness_logs,
    resolve_event_log_path,
};
use crate::agent::runloop::unified::run_loop_context::TurnRunId;

pub(super) fn initialize_harness(
    vt_cfg: Option<&VTCodeConfig>,
    model: &str,
    turn_run_id: &TurnRunId,
) -> Option<HarnessEventEmitter> {
    let harness_config = vt_cfg.map(|cfg| cfg.agent.harness.clone()).unwrap_or_default();
    let effective_log_path = harness_config
        .event_log_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .or_else(default_harness_log_dir);
    let Some(log_path) = effective_log_path else {
        tracing::warn!(target: "vtcode.harness", phase = "setup", error = "no log path", "harness setup skipped");
        return None;
    };

    let log_dir = if log_path.extension().is_some() {
        log_path.parent().unwrap_or(Path::new(".")).to_path_buf()
    } else {
        log_path.clone()
    };
    if let Err(error) = prune_old_harness_logs(&log_dir, HARNESS_LOG_MAX_AGE_DAYS) {
        tracing::warn!(target: "vtcode.harness", phase = "harness_prune", path = %log_dir.display(), error = %error, "harness log pruning failed");
    }

    let resolved_path = resolve_event_log_path(&log_path.to_string_lossy(), turn_run_id);
    let emitter = match HarnessEventEmitter::new(resolved_path.clone()) {
        Ok(emitter) => emitter,
        Err(error) => {
            tracing::warn!(target: "vtcode.harness", phase = "setup", path = %resolved_path.display(), error = %error, "harness emitter setup failed");
            return None;
        }
    };

    let features = FeatureSet::from_config(vt_cfg);
    if features.open_responses.emit_events {
        let open_responses_config = vt_cfg.map(|cfg| cfg.agent.open_responses.clone()).unwrap_or_default();
        let parent = log_path.parent().unwrap_or(Path::new("."));
        let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
        let output_path = parent.join(format!("open-responses-{}-{}.jsonl", turn_run_id.0, timestamp));
        if let Err(error) = emitter.enable_open_responses(open_responses_config, model, Some(output_path.clone())) {
            tracing::warn!(target: "vtcode.harness", phase = "open_responses_setup", path = %output_path.display(), error = %error, "Open Responses setup failed");
        }
    }

    if vt_cfg.is_some_and(|cfg| cfg.telemetry.atif_enabled) {
        let atif_path =
            log_dir.join(format!("atif-trajectory-{}-{}.json", turn_run_id.0, Utc::now().format("%Y%m%dT%H%M%SZ")));
        if let Err(error) = emitter.enable_atif(model, atif_path.clone()) {
            tracing::warn!(target: "vtcode.harness", phase = "atif_setup", path = %atif_path.display(), error = %error, "ATIF setup failed");
        }
    }

    if let Err(error) =
        emitter.emit(ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: turn_run_id.0.clone() }))
    {
        tracing::warn!(target: "vtcode.harness", phase = "thread_start", path = %resolved_path.display(), error = %error, "harness thread-start event failed");
    }
    Some(emitter)
}
