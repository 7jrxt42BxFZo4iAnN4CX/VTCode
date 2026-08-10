//! Typed storage boundary for OpenRouter OAuth tokens.
//!
//! New tokens use the shared [`CredentialStorage`] backends. The legacy
//! OpenRouter file format remains readable so existing installations migrate
//! on their next successful load.

use anyhow::{Context, Result, anyhow};
use base64::{Engine, engine::general_purpose::STANDARD};
use ring::aead::{self, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use std::fs;
use std::path::PathBuf;

use crate::credentials::{AuthCredentialsStoreMode, CredentialStorage};
use crate::storage_paths::auth_storage_dir;

use super::openrouter_oauth::OpenRouterToken;

const STORAGE_SERVICE: &str = "vtcode";
const STORAGE_USER: &str = "openrouter_oauth";
const LEGACY_TOKEN_FILE: &str = "openrouter.json";

/// Typed persistence boundary for an OpenRouter token.
pub(crate) struct OpenRouterTokenStorage {
    backend: CredentialStorage,
}

impl OpenRouterTokenStorage {
    pub(crate) fn new() -> Self {
        Self {
            backend: CredentialStorage::new(STORAGE_SERVICE, STORAGE_USER),
        }
    }

    pub(crate) fn save(&self, token: &OpenRouterToken, mode: AuthCredentialsStoreMode) -> Result<()> {
        self.backend
            .store_json_exact_with_mode(token, mode)
            .context("failed to persist openrouter token")
    }

    pub(crate) fn load(&self, mode: AuthCredentialsStoreMode) -> Result<Option<OpenRouterToken>> {
        if let Some(token) = self.backend.load_json_exact_with_mode(mode)? {
            return Ok(Some(token));
        }

        if mode.effective_mode() != AuthCredentialsStoreMode::File {
            return Ok(None);
        }

        let Some(token) = load_legacy_token()? else {
            return Ok(None);
        };

        self.save(&token, mode).context("failed to migrate legacy openrouter token")?;
        if let Err(err) = clear_legacy_token_file() {
            tracing::warn!("failed to remove migrated legacy openrouter token: {err}");
        }
        Ok(Some(token))
    }

    pub(crate) fn clear(&self, mode: AuthCredentialsStoreMode) -> Result<()> {
        let mut errors = Vec::new();
        let effective_mode = mode.effective_mode();
        if let Err(err) = self.backend.clear_exact_with_mode(effective_mode) {
            errors.push(err.to_string());
        }
        if effective_mode == AuthCredentialsStoreMode::File {
            if let Err(err) = clear_legacy_token_file() {
                errors.push(err.to_string());
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(anyhow!("failed to clear openrouter token: {}", errors.join("; ")))
        }
    }

    /// Clear both shared backends and the legacy file regardless of the
    /// configured default mode.
    pub(crate) fn clear_all(&self) -> Result<()> {
        let mut errors = Vec::new();
        for mode in [AuthCredentialsStoreMode::Keyring, AuthCredentialsStoreMode::File] {
            if let Err(err) = self.backend.clear_exact_with_mode(mode) {
                errors.push(err.to_string());
            }
        }
        if let Err(err) = clear_legacy_token_file() {
            errors.push(err.to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(anyhow!("failed to clear openrouter token: {}", errors.join("; ")))
        }
    }

    #[cfg(test)]
    pub(crate) fn current_file_path(&self) -> Result<PathBuf> {
        self.backend.file_path_for_tests()
    }
}

fn load_legacy_token() -> Result<Option<OpenRouterToken>> {
    let path = legacy_token_path()?;
    let data = match fs::read(&path) {
        Ok(data) => data,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(anyhow!("failed to read legacy openrouter token: {err}")),
    };

    let encrypted: LegacyEncryptedToken =
        serde_json::from_slice(&data).context("failed to decode legacy openrouter token")?;
    decrypt_legacy_token(&encrypted).map(Some)
}

fn clear_legacy_token_file() -> Result<()> {
    let path = legacy_token_path()?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(anyhow!("failed to delete legacy openrouter token: {err}")),
    }
}

pub(crate) fn legacy_token_path() -> Result<PathBuf> {
    Ok(auth_storage_dir()?.join(LEGACY_TOKEN_FILE))
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct LegacyEncryptedToken {
    nonce: String,
    ciphertext: String,
    version: u8,
}

pub(crate) fn encrypt_legacy_token(token: &OpenRouterToken) -> Result<LegacyEncryptedToken> {
    let key = derive_legacy_encryption_key()?;
    let rng = SystemRandom::new();
    let mut nonce_bytes = [0_u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes).map_err(|_| anyhow!("failed to generate nonce"))?;

    let mut ciphertext = serde_json::to_vec(token).context("failed to serialize openrouter token")?;
    key.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce_bytes), Aad::empty(), &mut ciphertext)
        .map_err(|_| anyhow!("failed to encrypt openrouter token"))?;

    Ok(LegacyEncryptedToken {
        nonce: STANDARD.encode(nonce_bytes),
        ciphertext: STANDARD.encode(ciphertext),
        version: 1,
    })
}

pub(crate) fn decrypt_legacy_token(encrypted: &LegacyEncryptedToken) -> Result<OpenRouterToken> {
    if encrypted.version != 1 {
        return Err(anyhow!("unsupported openrouter token format version: {}", encrypted.version));
    }

    let key = derive_legacy_encryption_key()?;
    let nonce_bytes: [u8; NONCE_LEN] = STANDARD
        .decode(&encrypted.nonce)
        .context("invalid openrouter token nonce encoding")?
        .try_into()
        .map_err(|_| anyhow!("invalid openrouter token nonce length"))?;
    let mut ciphertext = STANDARD
        .decode(&encrypted.ciphertext)
        .context("invalid openrouter token ciphertext encoding")?;
    let plaintext = key
        .open_in_place(Nonce::assume_unique_for_key(nonce_bytes), Aad::empty(), &mut ciphertext)
        .map_err(|_| anyhow!("failed to decrypt openrouter token"))?;
    serde_json::from_slice(plaintext).context("failed to deserialize openrouter token")
}

fn derive_legacy_encryption_key() -> Result<LessSafeKey> {
    use ring::digest::{SHA256, digest};

    let mut key_material = Vec::new();
    if let Ok(hostname) = hostname::get() {
        key_material.extend_from_slice(hostname.as_encoded_bytes());
    }

    #[cfg(unix)]
    {
        key_material.extend_from_slice(&nix::unistd::getuid().as_raw().to_le_bytes());
    }
    #[cfg(not(unix))]
    {
        if let Ok(user) = std::env::var("USER").or_else(|_| std::env::var("USERNAME")) {
            key_material.extend_from_slice(user.as_bytes());
        }
    }

    key_material.extend_from_slice(b"vtcode-openrouter-oauth-v1");
    let hash = digest(&SHA256, &key_material);
    let key_bytes: &[u8; 32] = hash
        .as_ref()
        .get(..32)
        .context("openrouter token encryption key was too short")?
        .try_into()
        .context("openrouter token encryption key had an invalid length")?;
    let unbound = UnboundKey::new(&aead::AES_256_GCM, key_bytes).map_err(|_| anyhow!("invalid encryption key"))?;
    Ok(LessSafeKey::new(unbound))
}
