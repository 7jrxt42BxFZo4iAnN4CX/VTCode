use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::Utc;

use crate::agent::runloop::unified::run_loop_context::TurnRunId;

/// Default maximum age for explicit legacy harness files.
pub(crate) const HARNESS_LOG_MAX_AGE_DAYS: u64 = 30;

const HARNESS_LOG_PREFIX: &str = "harness-";
const SECONDS_PER_DAY: u64 = 86400;

/// Prune only explicit legacy JSONL files in the supplied directory.
pub(crate) fn prune_old_harness_logs(log_dir: &Path, max_age_days: u64) -> Result<()> {
    if max_age_days == 0 {
        return Ok(());
    }

    let cutoff = match SystemTime::now().checked_sub(Duration::from_secs(max_age_days.saturating_mul(SECONDS_PER_DAY)))
    {
        Some(time) => time,
        None => return Ok(()),
    };
    let entries =
        fs::read_dir(log_dir).with_context(|| format!("failed to read harness log directory {}", log_dir.display()))?;

    for entry in entries {
        let entry =
            entry.with_context(|| format!("failed to enumerate harness log directory {}", log_dir.display()))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with(HARNESS_LOG_PREFIX) || !name.ends_with(".jsonl") {
            continue;
        }
        let modified = entry
            .metadata()
            .with_context(|| format!("failed to inspect harness log {}", path.display()))?
            .modified()
            .with_context(|| format!("failed to read modification time for harness log {}", path.display()))?;
        if modified <= cutoff {
            fs::remove_file(&path).with_context(|| format!("failed to prune harness log {}", path.display()))?;
        }
    }
    Ok(())
}

/// Resolve a configured legacy path, adding a unique file name for directories.
pub(crate) fn resolve_event_log_path(path: &str, run_id: &TurnRunId) -> PathBuf {
    let mut base = PathBuf::from(path);
    if base.extension().is_none() {
        let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
        base = base.join(format!("harness-{}-{}.jsonl", run_id.0, timestamp));
    }
    base
}
