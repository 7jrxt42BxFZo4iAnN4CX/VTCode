use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::path::Path;
use vtcode_commons::VtCodePaths;

use super::time::unix_timestamp_now;

pub(crate) fn load_json_cache<T: DeserializeOwned>(path: &Path, label: &str) -> Result<T> {
    let content = String::from_utf8(VtCodePaths::read_file_no_follow(path)?)
        .with_context(|| format!("Failed to read {} at {}", label, path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("Failed to parse {} at {}", label, path.display()))
}

pub(crate) fn save_json_cache<T: Serialize>(state_dir: &Path, path: &Path, value: &T, label: &str) -> Result<()> {
    VtCodePaths::ensure_user_dir(state_dir).with_context(|| format!("Failed to create {}", state_dir.display()))?;
    let bytes = serde_json::to_vec(value).with_context(|| format!("Failed to serialize {label}"))?;
    VtCodePaths::write_private_file_atomic(path, &bytes)
        .with_context(|| format!("Failed to write {} at {}", label, path.display()))?;
    Ok(())
}

pub(crate) fn cache_is_stale(last_attempt: u64, stale_after_secs: u64) -> bool {
    unix_timestamp_now().saturating_sub(last_attempt) > stale_after_secs
}
