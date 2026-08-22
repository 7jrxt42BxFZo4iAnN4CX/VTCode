use anyhow::{Context, Result};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[cfg(test)]
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use vtcode_commons::VtCodePaths;

#[cfg(test)]
static TEST_CACHE_DIR: LazyLock<Mutex<Option<PathBuf>>> = LazyLock::new(|| Mutex::new(None));

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct UpdateCacheSnapshot {
    pub(super) last_checked: Option<SystemTime>,
    pub(super) latest_version: Option<Version>,
    pub(super) latest_was_newer: bool,
    pub(super) last_seen_version: Option<Version>,
    pub(super) dismissed_version: Option<Version>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpdateCachePayload {
    last_checked_unix_secs: u64,
    #[serde(default)]
    latest_version: Option<String>,
    #[serde(default)]
    latest_was_newer: bool,
    #[serde(default)]
    last_seen_version: Option<String>,
    #[serde(default)]
    dismissed_version: Option<String>,
}

pub(super) fn read_snapshot() -> Result<UpdateCacheSnapshot> {
    let cache_file = cache_file_path()?;
    let metadata = match std::fs::symlink_metadata(&cache_file) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            anyhow::bail!("refusing to read symlinked update cache {}", cache_file.display())
        }
        Ok(metadata) if !metadata.is_file() => {
            anyhow::bail!("update cache is not a regular file: {}", cache_file.display())
        }
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(UpdateCacheSnapshot::default());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to inspect update cache {}", cache_file.display()));
        }
    };
    let modified = metadata.modified().ok();

    let content = String::from_utf8(VtCodePaths::read_file_no_follow(&cache_file)?)
        .with_context(|| format!("Failed to read update cache metadata {}", cache_file.display()))?;

    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(UpdateCacheSnapshot {
            last_checked: modified,
            latest_version: None,
            latest_was_newer: false,
            last_seen_version: None,
            dismissed_version: None,
        });
    }

    let Ok(payload) = serde_json::from_str::<UpdateCachePayload>(trimmed) else {
        return Ok(UpdateCacheSnapshot {
            last_checked: modified,
            latest_version: None,
            latest_was_newer: false,
            last_seen_version: None,
            dismissed_version: None,
        });
    };

    Ok(UpdateCacheSnapshot {
        last_checked: Some(UNIX_EPOCH + std::time::Duration::from_secs(payload.last_checked_unix_secs)).or(modified),
        latest_version: payload.latest_version.as_deref().and_then(|value| Version::parse(value).ok()),
        latest_was_newer: payload.latest_was_newer,
        last_seen_version: payload
            .last_seen_version
            .as_deref()
            .and_then(|value| Version::parse(value).ok()),
        dismissed_version: payload
            .dismissed_version
            .as_deref()
            .and_then(|value| Version::parse(value).ok()),
    })
}

pub(super) fn record_successful_check(latest_version: Option<&Version>, latest_was_newer: bool) -> Result<()> {
    let existing = read_snapshot().unwrap_or_default();
    write_snapshot(UpdateCacheSnapshot {
        last_checked: Some(SystemTime::now()),
        latest_version: latest_version.cloned(),
        latest_was_newer,
        last_seen_version: existing.last_seen_version,
        dismissed_version: existing.dismissed_version,
    })
}

pub(super) fn record_failed_check() -> Result<()> {
    let mut snapshot = read_snapshot()?;
    snapshot.last_checked = Some(SystemTime::now());
    write_snapshot(snapshot)
}

pub(super) fn record_seen_version(version: &Version) -> Result<()> {
    let mut snapshot = read_snapshot()?;
    snapshot.last_seen_version = Some(version.clone());
    write_snapshot(snapshot)
}

pub(super) fn record_dismissed_version(version: &Version) -> Result<()> {
    let mut snapshot = read_snapshot()?;
    snapshot.dismissed_version = Some(version.clone());
    write_snapshot(snapshot)
}

pub(super) fn clear_dismissed_version() -> Result<()> {
    let mut snapshot = read_snapshot()?;
    snapshot.dismissed_version = None;
    write_snapshot(snapshot)
}

fn write_snapshot(snapshot: UpdateCacheSnapshot) -> Result<()> {
    let last_checked = snapshot.last_checked.unwrap_or_else(SystemTime::now);
    let last_checked_unix_secs = last_checked.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let payload = UpdateCachePayload {
        last_checked_unix_secs,
        latest_version: snapshot.latest_version.map(|version| version.to_string()),
        latest_was_newer: snapshot.latest_was_newer,
        last_seen_version: snapshot.last_seen_version.map(|version| version.to_string()),
        dismissed_version: snapshot.dismissed_version.map(|version| version.to_string()),
    };
    let serialized = serde_json::to_string(&payload).context("Failed to serialize update cache payload")?;
    let cache_file = cache_file_path()?;
    if let Some(parent) = cache_file.parent() {
        VtCodePaths::ensure_user_dir(parent).context("Failed to create update cache directory")?;
    }
    VtCodePaths::write_private_file_atomic(&cache_file, serialized.as_bytes())
        .with_context(|| format!("Failed to write update cache {}", cache_file.display()))?;
    Ok(())
}

fn get_cache_dir() -> Result<PathBuf> {
    #[cfg(test)]
    if let Some(path) = TEST_CACHE_DIR.lock().unwrap_or_else(|error| error.into_inner()).clone() {
        return VtCodePaths::ensure_user_dir(path);
    }

    Ok(VtCodePaths::resolve()?.ensure_cache_dir()?.to_path_buf())
}

#[cfg(test)]
pub(super) fn set_cache_dir_override_for_tests(path: Option<PathBuf>) -> Option<PathBuf> {
    let mut guard = TEST_CACHE_DIR.lock().unwrap_or_else(|error| error.into_inner());
    std::mem::replace(&mut *guard, path)
}

fn cache_file_path() -> Result<PathBuf> {
    Ok(get_cache_dir()?.join("last_update_check"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn empty_legacy_cache_file_uses_file_metadata() {
        let temp_dir = TempDir::new().expect("temp dir");
        let previous = set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

        let cache_file = cache_file_path().expect("cache path");
        std::fs::write(&cache_file, "").expect("write legacy cache");

        let snapshot = read_snapshot().expect("read snapshot");
        assert!(snapshot.last_checked.is_some());
        assert!(snapshot.latest_version.is_none());
        assert!(!snapshot.latest_was_newer);

        set_cache_dir_override_for_tests(previous);
    }

    #[test]
    fn json_cache_round_trips_latest_version() {
        let temp_dir = TempDir::new().expect("temp dir");
        let previous = set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

        let version = Version::parse("0.113.0").expect("version");
        record_successful_check(Some(&version), true).expect("write cache");

        let snapshot = read_snapshot().expect("read snapshot");
        assert_eq!(snapshot.latest_version, Some(version));
        assert!(snapshot.latest_was_newer);
        assert!(snapshot.last_checked.is_some());
        assert!(snapshot.dismissed_version.is_none());

        set_cache_dir_override_for_tests(previous);
    }

    #[test]
    fn record_and_clear_dismissed_version() {
        let temp_dir = TempDir::new().expect("temp dir");
        let previous = set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

        let version = Version::parse("0.113.0").expect("version");
        record_successful_check(Some(&version), true).expect("write cache");
        assert!(read_snapshot().expect("snapshot").dismissed_version.is_none());

        record_dismissed_version(&version).expect("record dismissal");
        let snapshot = read_snapshot().expect("read snapshot");
        assert_eq!(snapshot.dismissed_version, Some(version));

        clear_dismissed_version().expect("clear dismissal");
        assert!(read_snapshot().expect("snapshot").dismissed_version.is_none());

        set_cache_dir_override_for_tests(previous);
    }
}
