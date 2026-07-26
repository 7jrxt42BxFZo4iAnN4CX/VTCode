//! Credential storage — keyring and encrypted-file backends.
//!
//! # Module Structure
//!
//! | Submodule | Responsibility |
//! |---|---|
//! | mode | Backend selection enum (`Keyring` / `File` / `Auto`) |
//! | keyring | OS keyring creation, liveness, disable detection |
//! | encryption | AES-256-GCM encrypt/decrypt (pure, no IO) |
//! | storage | `CredentialStorage` — orchestrates backends |
//! | legacy | Legacy `auth.json` migration |

pub(crate) mod encryption;
pub(crate) mod keyring;
pub(crate) mod legacy;
pub(crate) mod mode;
pub(crate) mod storage;

pub use mode::AuthCredentialsStoreMode;
pub use storage::CredentialStorage;

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};

/// Custom API Key storage for provider-specific keys.
///
/// Provides secure storage and retrieval of API keys for custom providers
/// using the OS keyring or encrypted file storage.
pub struct CustomApiKeyStorage {
    provider: String,
    storage: CredentialStorage,
}

impl CustomApiKeyStorage {
    /// Create a new custom API key storage for a specific provider.
    pub fn new(provider: &str) -> Self {
        let normalized_provider = provider.trim().to_lowercase();
        Self {
            provider: normalized_provider.clone(),
            storage: CredentialStorage::new("vtcode", format!("api_key_{normalized_provider}")),
        }
    }

    /// Store an API key securely.
    pub fn store(&self, api_key: &str, mode: AuthCredentialsStoreMode) -> Result<()> {
        let api_key = api_key.trim();
        if api_key.is_empty() {
            bail!("API key cannot be empty");
        }
        self.storage.store_with_mode(api_key, mode)?;
        let persisted = self
            .storage
            .load_with_mode(mode)
            .context("failed to verify persisted API key")?;
        if persisted.as_deref() != Some(api_key) {
            bail!("secure storage did not return the API key after saving");
        }
        legacy::clear_for_provider(&self.provider)
            .context("failed to remove legacy plaintext credential after secure save")?;
        Ok(())
    }

    /// Retrieve a stored API key.
    pub fn load(&self, mode: AuthCredentialsStoreMode) -> Result<Option<String>> {
        if let Some(key) = self.storage.load_with_mode(mode)? {
            let key = key.trim();
            return Ok((!key.is_empty()).then(|| key.to_owned()));
        }

        self.load_legacy_auth_json(mode)
    }

    /// Clear (delete) a stored API key.
    pub fn clear(&self, mode: AuthCredentialsStoreMode) -> Result<()> {
        self.storage.clear_with_mode(mode)?;
        legacy::clear_for_provider(&self.provider).context("failed to remove legacy plaintext credential")?;
        Ok(())
    }

    fn load_legacy_auth_json(&self, mode: AuthCredentialsStoreMode) -> Result<Option<String>> {
        let Some(legacy_entry) = legacy::load_for_provider(&self.provider)? else {
            return Ok(None);
        };

        if let Err(err) = self.store(&legacy_entry.api_key, mode) {
            tracing::warn!(
                "Failed to migrate legacy plaintext auth.json entry for provider '{}' into secure storage: {}",
                self.provider,
                err
            );
            return Err(err).context("failed to migrate legacy API key into secure storage");
        }

        let path = crate::storage_paths::legacy_auth_storage_path().ok();
        if let Some(p) = path {
            legacy::delete_file(&p).context("failed to remove migrated plaintext auth file")?;
        }

        tracing::warn!(
            "Migrated legacy plaintext auth.json entry for provider '{}' into secure storage",
            self.provider
        );
        self.load(mode)
    }
}

/// Migrate plain-text API keys from a config map into secure storage.
///
/// Returns a map of provider → success/failure.
pub fn migrate_custom_api_keys(
    custom_api_keys: &BTreeMap<String, String>,
    mode: AuthCredentialsStoreMode,
) -> Result<BTreeMap<String, bool>> {
    let mut results = BTreeMap::new();

    for (provider, api_key) in custom_api_keys {
        let storage = CustomApiKeyStorage::new(provider);
        match storage.store(api_key, mode) {
            Ok(()) => {
                tracing::info!("Migrated API key for provider '{provider}' to secure storage");
                results.insert(provider.clone(), true);
            }
            Err(e) => {
                tracing::warn!("Failed to migrate API key for provider '{provider}': {e}");
                results.insert(provider.clone(), false);
            }
        }
    }

    Ok(results)
}

/// Load all custom API keys from secure storage.
///
/// Returns a map of provider → API key for those that have stored keys.
pub fn load_custom_api_keys(providers: &[String], mode: AuthCredentialsStoreMode) -> Result<BTreeMap<String, String>> {
    let mut api_keys = BTreeMap::new();

    for provider in providers {
        let storage = CustomApiKeyStorage::new(provider);
        if let Some(key) = storage.load(mode)? {
            api_keys.insert(provider.clone(), key);
        }
    }

    Ok(api_keys)
}

/// Clear all custom API keys from secure storage.
pub fn clear_custom_api_keys(providers: &[String], mode: AuthCredentialsStoreMode) -> Result<()> {
    for provider in providers {
        let storage = CustomApiKeyStorage::new(provider);
        if let Err(e) = storage.clear(mode) {
            tracing::warn!("Failed to clear API key for provider '{provider}': {e}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::path::PathBuf;
    use tempfile::TempDir;

    struct TestAuthDirGuard {
        temp_dir: Option<TempDir>,
        previous: Option<PathBuf>,
    }

    impl TestAuthDirGuard {
        fn new() -> Self {
            let temp_dir = TempDir::new().expect("create temp auth dir");
            let previous = crate::storage_paths::auth_storage_dir_override_for_tests().expect("read auth dir override");
            crate::storage_paths::set_auth_storage_dir_override_for_tests(Some(temp_dir.path().to_path_buf()))
                .expect("set temp auth dir override");
            Self { temp_dir: Some(temp_dir), previous }
        }
    }

    impl Drop for TestAuthDirGuard {
        fn drop(&mut self) {
            crate::storage_paths::set_auth_storage_dir_override_for_tests(self.previous.clone())
                .expect("restore auth dir override");
            if let Some(temp_dir) = self.temp_dir.take() {
                temp_dir.close().expect("remove temp auth dir");
            }
        }
    }

    #[test]
    #[serial]
    #[cfg(unix)]
    fn file_api_key_storage_round_trips_with_private_permissions() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let guard = TestAuthDirGuard::new();
        let storage = CustomApiKeyStorage::new("stepfun");
        storage
            .store("test-stepfun-key", AuthCredentialsStoreMode::File)
            .expect("store API key");
        assert_eq!(
            storage.load(AuthCredentialsStoreMode::File).expect("load API key").as_deref(),
            Some("test-stepfun-key")
        );

        let auth_dir = guard.temp_dir.as_ref().expect("test auth dir").path();
        assert_eq!(fs::metadata(auth_dir).expect("auth dir metadata").permissions().mode() & 0o777, 0o700);
        let credential_file = fs::read_dir(auth_dir)
            .expect("read auth dir")
            .map(|entry| entry.expect("credential entry").path())
            .find(|path| path.extension().and_then(|extension| extension.to_str()) == Some("json"))
            .expect("credential file");
        assert_eq!(fs::metadata(credential_file).expect("credential metadata").permissions().mode() & 0o777, 0o600);
    }

    #[test]
    #[serial]
    fn keyring_mode_falls_back_to_encrypted_file_when_keyring_is_unavailable() {
        let _guard = TestAuthDirGuard::new();
        let storage = CustomApiKeyStorage::new("stepfun");

        storage
            .store("test-stepfun-key", AuthCredentialsStoreMode::Keyring)
            .expect("keyring mode should fall back to encrypted file storage");
        assert_eq!(
            storage
                .load(AuthCredentialsStoreMode::Keyring)
                .expect("load API key")
                .as_deref(),
            Some("test-stepfun-key")
        );
    }
}
