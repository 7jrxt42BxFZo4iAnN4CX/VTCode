//! Strict storage boundary for the interactive secret manager.
//!
//! UI code should depend on [`SecretStorage`] rather than constructing auth
//! backends directly. This keeps provider validation, normalization, and the
//! configured backend selection in one independently testable interface.

use anyhow::{Context, Result, bail};
use vtcode_auth::{AuthCredentialsStoreMode, CustomApiKeyStorage};
use vtcode_core::config::models::Provider;

pub(super) struct SecretStorage {
    mode: AuthCredentialsStoreMode,
}

impl SecretStorage {
    pub(super) fn new(mode: AuthCredentialsStoreMode) -> Self {
        Self { mode }
    }

    pub(super) fn validate_provider(provider: Provider) -> Result<()> {
        validate_api_key_provider(provider)
    }

    pub(super) fn load(&self, provider: Provider) -> Result<Option<String>> {
        validate_api_key_provider(provider)?;
        CustomApiKeyStorage::new(provider.as_ref())
            .load(self.mode)
            .with_context(|| format!("failed to load {} credential", provider.label()))
    }

    pub(super) fn store(&self, provider: Provider, value: &str) -> Result<()> {
        validate_api_key_provider(provider)?;
        let value = value.trim();
        if value.is_empty() {
            bail!("API key cannot be empty");
        }

        CustomApiKeyStorage::new(provider.as_ref())
            .store(value, self.mode)
            .with_context(|| format!("failed to store {} credential", provider.label()))
    }

    pub(super) fn clear(&self, provider: Provider) -> Result<()> {
        validate_api_key_provider(provider)?;
        CustomApiKeyStorage::new(provider.as_ref())
            .clear(self.mode)
            .with_context(|| format!("failed to clear {} credential", provider.label()))
    }
}

fn validate_api_key_provider(provider: Provider) -> Result<()> {
    if provider.is_local() {
        bail!("{} is a local provider and does not use an API key", provider.label());
    }
    if provider.uses_managed_auth() {
        bail!("{} uses managed authentication; use its login flow", provider.label());
    }
    Ok(())
}
