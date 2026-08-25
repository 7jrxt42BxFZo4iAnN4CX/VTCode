use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};

use crate::api_keys::{api_key_env_var, credential_metadata_key, store_credential_with_mode};
use crate::defaults::{self};
use crate::hooks::{HooksConfig, LifecycleHooksConfig, WorkspaceHookCommand, WorkspaceLifecycleHooks};
use crate::loader::config::VTCodeConfig;
use crate::loader::layers::{
    ConfigLayerEntry, ConfigLayerMetadata, ConfigLayerSource, ConfigLayerStack, LayerDisabledReason,
};
use vtcode_commons::VtCodePaths;
use vtcode_commons::canonicalize;

type CachedManager = Arc<ConfigManager>;

#[cfg(not(test))]
static WORKSPACE_CACHE: Mutex<Option<HashMap<PathBuf, CachedManager>>> = Mutex::new(None);

#[cfg(not(test))]
fn with_cache_mut(f: impl FnOnce(&mut HashMap<PathBuf, CachedManager>)) {
    let mut guard = WORKSPACE_CACHE.lock().expect("config cache lock poisoned");
    guard.get_or_insert_with(HashMap::new);
    f(guard.as_mut().expect("cache initialized"))
}

#[cfg(not(test))]
fn cache_get(workspace: &Path) -> Option<CachedManager> {
    WORKSPACE_CACHE
        .lock()
        .expect("config cache lock poisoned")
        .as_ref()
        .and_then(|map| map.get(workspace).cloned())
}

#[cfg(not(test))]
fn cache_insert(workspace: PathBuf, manager: CachedManager) {
    with_cache_mut(move |map| {
        map.insert(workspace, manager);
    });
}

#[cfg(not(test))]
fn cache_remove(workspace: &Path) {
    with_cache_mut(|map| {
        map.remove(workspace);
    });
}

fn canonicalize_workspace_root(path: &Path) -> PathBuf {
    canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn push_unique_layer_source(layer_sources: &mut Vec<ConfigLayerSource>, source: ConfigLayerSource) {
    let file = match &source {
        ConfigLayerSource::System { file }
        | ConfigLayerSource::User { file }
        | ConfigLayerSource::Project { file }
        | ConfigLayerSource::Workspace { file } => file,
        ConfigLayerSource::Runtime => {
            layer_sources.push(source);
            return;
        }
    };
    if !layer_sources.iter().any(|existing| {
        matches!(existing,
            ConfigLayerSource::System { file: existing_file }
            | ConfigLayerSource::User { file: existing_file }
            | ConfigLayerSource::Project { file: existing_file }
            | ConfigLayerSource::Workspace { file: existing_file }
            if existing_file == file)
    }) {
        layer_sources.push(source);
    }
}

fn is_optional_global_layer(source: &ConfigLayerSource) -> bool {
    matches!(source, ConfigLayerSource::System { .. } | ConfigLayerSource::User { .. })
}

fn should_skip_optional_global_error(source: &ConfigLayerSource, error: &std::io::Error) -> bool {
    is_optional_global_layer(source) && error.kind() != std::io::ErrorKind::InvalidData
}

fn ensure_private_parent_dir(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let _ = VtCodePaths::ensure_user_dir(parent)
        .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    Ok(())
}

/// Timing metrics (in microseconds) for configuration loading phases
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConfigPhaseTiming {
    /// Duration of workspace path resolution (in microseconds)
    pub(crate) path_resolution_us: u64,
    /// Duration of layer probing and loading (in microseconds)
    pub(crate) layer_loading_us: u64,
    /// Duration of TOML merging and deserialization (in microseconds)
    merge_and_parse_us: u64,
    /// Duration of validation and API key migration (in microseconds)
    pub(crate) validation_us: u64,
}

/// Configuration manager for loading and validating configurations
#[derive(Clone)]
pub struct ConfigManager {
    pub(crate) config: VTCodeConfig,
    config_path: Option<PathBuf>,
    canonical_user_config_path: Option<PathBuf>,
    tracked_user_config_paths: Vec<PathBuf>,
    write_global_config_to_canonical: bool,
    workspace_root: Option<PathBuf>,
    config_file_name: String,
    pub(crate) layer_stack: ConfigLayerStack,
    phase_timing: Option<ConfigPhaseTiming>,
}

impl ConfigManager {
    /// Load configuration from the default locations rooted at the current directory.
    pub fn load() -> Result<Self> {
        if let Ok(config_path) = std::env::var("VTCODE_CONFIG_PATH") {
            let trimmed = config_path.trim();
            if !trimmed.is_empty() {
                return Self::load_from_file_impl(trimmed, true)
                    .with_context(|| format!("Failed to load configuration from VTCODE_CONFIG_PATH={trimmed}"));
            }
        }

        Self::load_from_workspace(std::env::current_dir()?)
    }

    /// Load only the system and user configuration layers.
    ///
    /// This is used by global configuration commands that must honor legacy
    /// and XDG search precedence without accidentally importing the current
    /// workspace's `vtcode.toml`.
    pub fn load_global() -> Result<Self> {
        let defaults_provider = defaults::current_config_defaults();
        let config_file_name = defaults_provider.config_file_name().to_string();
        let canonical_user_config_path = defaults_provider.canonical_user_config_path(&config_file_name)?;
        let mut tracked_user_config_paths = defaults_provider.home_config_paths(&config_file_name);
        if let Some(path) = &canonical_user_config_path
            && !tracked_user_config_paths.iter().any(|existing| existing == path)
        {
            tracked_user_config_paths.push(path.clone());
        }

        let mut layer_sources = Vec::new();
        for system_config_path in defaults_provider.system_config_paths(&config_file_name)? {
            push_unique_layer_source(&mut layer_sources, ConfigLayerSource::System { file: system_config_path });
        }
        for home_config_path in &tracked_user_config_paths {
            push_unique_layer_source(&mut layer_sources, ConfigLayerSource::User { file: home_config_path.clone() });
        }

        let mut layer_stack = ConfigLayerStack::default();
        for source in layer_sources {
            if let Some(layer) = Self::load_optional_layer(source) {
                layer_stack.push(layer);
            }
        }

        if let Some((layer, error)) = layer_stack.first_layer_error() {
            bail!("Configuration layer '{}' failed to load: {}", layer.source.label(), error.message);
        }

        let (config, config_path) = if layer_stack.layers().is_empty() {
            let config = VTCodeConfig::default();
            config.validate().context("Default configuration failed validation")?;
            (config, None)
        } else {
            let (effective_toml, origins) = layer_stack.effective_config_with_origins();
            let mut config: VTCodeConfig = effective_toml
                .try_into()
                .context("Failed to deserialize effective global configuration")?;
            Self::validate_restricted_agent_fields(&layer_stack, &origins)?;
            config.validate().context("Global configuration failed validation")?;
            config.workspace_lifecycle_hooks =
                Some(Self::collect_workspace_lifecycle_hooks(&layer_stack, &config.hooks));
            let config_path = layer_stack
                .layers()
                .iter()
                .rev()
                .find(|layer| layer.is_enabled())
                .and_then(|layer| match &layer.source {
                    ConfigLayerSource::System { file } | ConfigLayerSource::User { file } => Some(file.clone()),
                    _ => None,
                });
            (config, config_path)
        };

        Ok(Self {
            config,
            config_path,
            canonical_user_config_path,
            tracked_user_config_paths,
            write_global_config_to_canonical: false,
            workspace_root: None,
            config_file_name,
            layer_stack,
            phase_timing: None,
        })
    }

    /// Invalidate the cached configuration for a specific workspace.
    ///
    /// Call this when config files may have changed on disk and the next
    /// `load_from_workspace` call should perform a fresh read instead of
    /// returning a previously cached result.
    pub fn invalidate_workspace_cache(workspace: impl AsRef<Path>) {
        let workspace = workspace.as_ref();
        #[cfg(not(test))]
        {
            let canonical_workspace = canonicalize_workspace_root(workspace);
            cache_remove(&canonical_workspace);
        }
        #[cfg(test)]
        let _ = workspace;
    }

    /// Load configuration from a specific workspace
    pub fn load_from_workspace(workspace: impl AsRef<Path>) -> Result<Self> {
        let workspace = workspace.as_ref();
        #[cfg(not(test))]
        let canonical_workspace = canonicalize_workspace_root(workspace);

        #[cfg(not(test))]
        if let Some(cached) = cache_get(&canonical_workspace) {
            return Ok(cached.as_ref().clone());
        }

        let manager = Self::load_from_workspace_impl(workspace)?;

        #[cfg(not(test))]
        cache_insert(canonical_workspace, Arc::new(manager.clone()));

        Ok(manager)
    }

    fn load_from_workspace_impl(workspace: impl AsRef<Path>) -> Result<Self> {
        let t0 = std::time::Instant::now();
        let workspace = workspace.as_ref();
        let defaults_provider = defaults::current_config_defaults();
        let workspace_paths = defaults_provider.workspace_paths_for(workspace);
        let workspace_root = canonicalize_workspace_root(workspace_paths.workspace_root());
        let config_dir = workspace_paths.config_dir();
        let config_file_name = defaults_provider.config_file_name().to_string();
        let canonical_user_config_path = defaults_provider.canonical_user_config_path(&config_file_name)?;
        let mut tracked_user_config_paths = defaults_provider.home_config_paths(&config_file_name);
        if let Some(path) = &canonical_user_config_path
            && !tracked_user_config_paths.iter().any(|existing| existing == path)
        {
            tracked_user_config_paths.push(path.clone());
        }
        let path_res_duration = t0.elapsed();

        let t1 = std::time::Instant::now();

        // Collect layer sources in precedence order so we can load them
        // concurrently and then push results while preserving order.
        let mut layer_sources: Vec<ConfigLayerSource> = Vec::with_capacity(8);

        // 1. System configuration: /etc compatibility followed by
        // XDG_CONFIG_DIRS candidates, all low-to-high precedence.
        for system_config_path in defaults_provider.system_config_paths(&config_file_name)? {
            push_unique_layer_source(&mut layer_sources, ConfigLayerSource::System { file: system_config_path });
        }

        // 2. Legacy user config followed by the canonical XDG user config.
        for home_config_path in &tracked_user_config_paths {
            push_unique_layer_source(&mut layer_sources, ConfigLayerSource::User { file: home_config_path.clone() });
        }

        // 3. Project-specific config (.vtcode/projects/<project>/config/vtcode.toml)
        if let Some(project_config_path) = Self::project_config_path(&config_dir, &workspace_root, &config_file_name) {
            push_unique_layer_source(&mut layer_sources, ConfigLayerSource::Project { file: project_config_path });
        }

        // 4. Config directory fallback (.vtcode/vtcode.toml)
        let fallback_path = config_dir.join(&config_file_name);
        let workspace_config_path = workspace_root.join(&config_file_name);
        if fallback_path != workspace_config_path {
            push_unique_layer_source(&mut layer_sources, ConfigLayerSource::Workspace { file: fallback_path });
        }

        // 5. Workspace config (vtcode.toml in workspace root)
        push_unique_layer_source(
            &mut layer_sources,
            ConfigLayerSource::Workspace { file: workspace_config_path.clone() },
        );

        // Load all layers concurrently. Each load is independent I/O, so
        // spawning threads overlaps the disk reads and canonicalization
        // syscalls. The results are collected in source order to preserve
        // layer precedence.
        let raw_layers: Vec<Option<ConfigLayerEntry>> = std::thread::scope(|s| {
            let handles = layer_sources
                .into_iter()
                .map(|source| s.spawn(move || Self::load_optional_layer(source)))
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|h| h.join().expect("config layer thread panicked"))
                .collect()
        });

        let mut layer_stack = ConfigLayerStack::default();
        for layer in raw_layers.into_iter().flatten() {
            layer_stack.push(layer);
        }

        let layer_load_duration = t1.elapsed();

        // If no layers found, use default config
        if layer_stack.layers().is_empty() {
            let t_val = std::time::Instant::now();
            let config = VTCodeConfig::default();
            config.validate().context("Default configuration failed validation")?;
            let val_duration = t_val.elapsed();

            let phase_timing = ConfigPhaseTiming {
                path_resolution_us: path_res_duration.as_micros() as u64,
                layer_loading_us: layer_load_duration.as_micros() as u64,
                merge_and_parse_us: 0,
                validation_us: val_duration.as_micros() as u64,
            };
            tracing::debug!(target: "vtcode_config", ?phase_timing, "Default configuration loaded");

            return Ok(Self {
                config,
                config_path: None,
                canonical_user_config_path,
                tracked_user_config_paths,
                write_global_config_to_canonical: false,
                workspace_root: Some(workspace_root),
                config_file_name,
                layer_stack,
                phase_timing: Some(phase_timing),
            });
        }

        // If the workspace root vtcode.toml sets workspace.use_root_config = true,
        // discard all lower-precedence layers and re-merge with only the workspace
        // root and runtime layers.
        let use_root_config = layer_stack
            .layers()
            .iter()
            .find(|l| {
                matches!(
                    &l.source,
                    ConfigLayerSource::Workspace { file }
                        if *file == workspace_config_path
                )
            })
            .map(|l| Self::workspace_root_wants_root_config_only(&l.config))
            .unwrap_or(false);
        if use_root_config {
            layer_stack.retain(|layer| {
                matches!(
                    &layer.source,
                    ConfigLayerSource::Workspace { file }
                        if *file == workspace_config_path
                ) || matches!(&layer.source, ConfigLayerSource::Runtime)
            });
            if layer_stack.layers().is_empty() {
                bail!(
                    "workspace.use_root_config is true but no workspace root config was found at {}",
                    workspace_config_path.display()
                );
            }
        }

        if let Some((layer, error)) = layer_stack.first_layer_error() {
            bail!("Configuration layer '{}' failed to load: {}", layer.source.label(), error.message);
        }

        let t2 = std::time::Instant::now();
        let (effective_toml, origins) = layer_stack.effective_config_with_origins();
        let mut config: VTCodeConfig = effective_toml
            .try_into()
            .context("Failed to deserialize effective configuration")?;
        let merge_duration = t2.elapsed();

        let t3 = std::time::Instant::now();
        Self::validate_restricted_agent_fields(&layer_stack, &origins)?;

        config.validate().context("Configuration failed validation")?;
        config.workspace_lifecycle_hooks = Some(Self::collect_workspace_lifecycle_hooks(&layer_stack, &config.hooks));

        // Migrate any plain-text API keys from config to secure storage
        migrate_custom_api_keys_if_needed(&mut config)?;
        let val_duration = t3.elapsed();

        let phase_timing = ConfigPhaseTiming {
            path_resolution_us: path_res_duration.as_micros() as u64,
            layer_loading_us: layer_load_duration.as_micros() as u64,
            merge_and_parse_us: merge_duration.as_micros() as u64,
            validation_us: val_duration.as_micros() as u64,
        };
        tracing::debug!(target: "vtcode_config", ?phase_timing, "Workspace configuration loaded");

        let config_path = layer_stack
            .layers()
            .iter()
            .rev()
            .find(|layer| layer.is_enabled())
            .and_then(|l| match &l.source {
                ConfigLayerSource::User { file } => Some(file.clone()),
                ConfigLayerSource::Project { file } => Some(file.clone()),
                ConfigLayerSource::Workspace { file } => Some(file.clone()),
                ConfigLayerSource::System { file } => Some(file.clone()),
                ConfigLayerSource::Runtime => None,
            });

        Ok(Self {
            config,
            config_path,
            canonical_user_config_path,
            tracked_user_config_paths,
            write_global_config_to_canonical: false,
            workspace_root: Some(workspace_root),
            config_file_name,
            layer_stack,
            phase_timing: Some(phase_timing),
        })
    }

    fn load_toml_from_file(path: &Path) -> Result<toml::Value> {
        let content =
            fs::read_to_string(path).with_context(|| format!("Failed to read config file: {}", path.display()))?;
        let value: toml::Value =
            toml::from_str(&content).with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        Ok(value)
    }

    fn load_optional_layer(source: ConfigLayerSource) -> Option<ConfigLayerEntry> {
        let file = match &source {
            ConfigLayerSource::System { file }
            | ConfigLayerSource::User { file }
            | ConfigLayerSource::Project { file }
            | ConfigLayerSource::Workspace { file } => file,
            ConfigLayerSource::Runtime => {
                return Some(ConfigLayerEntry::new(source, toml::Value::Table(toml::Table::new())));
            }
        };

        // Read first so missing files return None without a redundant canonicalize.
        let content = match fs::read_to_string(file) {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
            Err(err) if should_skip_optional_global_error(&source, &err) => {
                tracing::debug!(path = %file.display(), "skipping inaccessible optional configuration layer");
                return None;
            }
            Err(err) => {
                let resolved_file = canonicalize_workspace_root(file);
                let resolved_source = source.with_file(resolved_file);
                return Some(Self::disabled_layer_from_error(resolved_source, err.into()));
            }
        };

        let resolved_file = canonicalize_workspace_root(file);
        let resolved_source = source.with_file(resolved_file);

        match toml::from_str::<toml::Value>(&content) {
            Ok(toml) => Some(ConfigLayerEntry::new(resolved_source, toml)),
            Err(err) => {
                let error =
                    anyhow::Error::from(err).context(format!("Failed to parse config file: {}", file.display()));
                Some(Self::disabled_layer_from_error(resolved_source, error))
            }
        }
    }

    fn disabled_layer_from_error(source: ConfigLayerSource, error: anyhow::Error) -> ConfigLayerEntry {
        let reason = if error.to_string().contains("parse") {
            LayerDisabledReason::ParseError
        } else {
            LayerDisabledReason::LoadError
        };
        ConfigLayerEntry::disabled(source, reason, format!("{error:#}"))
    }

    /// Check whether a parsed TOML value has `workspace.use_root_config = true`.
    ///
    /// This is checked against the already-loaded workspace root layer config
    /// to avoid a redundant file read.
    fn workspace_root_wants_root_config_only(config: &toml::Value) -> bool {
        config
            .get("workspace")
            .and_then(|v| v.get("use_root_config"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Load configuration from a specific file
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        Self::load_from_file_impl(path, false)
    }

    fn load_from_file_impl(path: impl AsRef<Path>, write_global_config_to_canonical: bool) -> Result<Self> {
        let path = path.as_ref();
        let defaults_provider = defaults::current_config_defaults();
        // Global layer probing always uses the canonical config file name, never
        // the basename of the explicit override file. Deriving it from the file
        // previously caused global layers (e.g. `~/.config/vtcode/vtcode.toml`
        // with `[[custom_providers]]`) to be skipped whenever the override file
        // was named something else (e.g. `night.toml`), silently dropping
        // custom providers and other user-global settings.
        let config_file_name = defaults_provider.config_file_name().to_string();
        let canonical_user_config_path = defaults_provider.canonical_user_config_path(&config_file_name)?;
        let mut tracked_user_config_paths = defaults_provider.home_config_paths(&config_file_name);
        if let Some(path) = &canonical_user_config_path
            && !tracked_user_config_paths.iter().any(|existing| existing == path)
        {
            tracked_user_config_paths.push(path.clone());
        }

        let mut layer_stack = ConfigLayerStack::default();
        let mut global_sources = Vec::new();

        for system_config in defaults_provider.system_config_paths(&config_file_name)? {
            push_unique_layer_source(&mut global_sources, ConfigLayerSource::System { file: system_config });
        }
        for home_config_path in &tracked_user_config_paths {
            push_unique_layer_source(&mut global_sources, ConfigLayerSource::User { file: home_config_path.clone() });
        }
        for source in global_sources {
            if let Some(layer) = Self::load_optional_layer(source) {
                layer_stack.push(layer);
            }
        }

        // 3. The specific file provided (Workspace layer)
        let file = path.to_path_buf();
        match Self::load_toml_from_file(path) {
            Ok(toml) => layer_stack.push(ConfigLayerEntry::new(ConfigLayerSource::Workspace { file }, toml)),
            Err(error) => {
                layer_stack.push(Self::disabled_layer_from_error(ConfigLayerSource::Workspace { file }, error))
            }
        }

        // If the provided file sets workspace.use_root_config = true, discard
        // lower-precedence layers so only this file and runtime overrides apply.
        let use_root_config = layer_stack
            .layers()
            .iter()
            .find(|l| {
                matches!(
                    &l.source,
                    ConfigLayerSource::Workspace { file }
                        if *file == path
                )
            })
            .map(|l| Self::workspace_root_wants_root_config_only(&l.config))
            .unwrap_or(false);
        if use_root_config {
            layer_stack.retain(|layer| {
                matches!(
                    &layer.source,
                    ConfigLayerSource::Workspace { file }
                        if *file == path
                ) || matches!(&layer.source, ConfigLayerSource::Runtime)
            });
        }

        if let Some((layer, error)) = layer_stack.first_layer_error() {
            bail!("Configuration layer '{}' failed to load: {}", layer.source.label(), error.message);
        }

        let (effective_toml, origins) = layer_stack.effective_config_with_origins();
        let mut config: VTCodeConfig = effective_toml
            .try_into()
            .with_context(|| format!("Failed to parse effective config with file: {}", path.display()))?;
        Self::validate_restricted_agent_fields(&layer_stack, &origins)?;

        config
            .validate()
            .with_context(|| format!("Failed to validate effective config with file: {}", path.display()))?;
        config.workspace_lifecycle_hooks = Some(Self::collect_workspace_lifecycle_hooks(&layer_stack, &config.hooks));

        Ok(Self {
            config,
            config_path: Some(canonicalize_workspace_root(path)),
            canonical_user_config_path,
            tracked_user_config_paths,
            write_global_config_to_canonical,
            workspace_root: path.parent().map(canonicalize_workspace_root),
            config_file_name,
            layer_stack,
            phase_timing: None,
        })
    }

    /// Get the loaded configuration
    pub fn config(&self) -> &VTCodeConfig {
        &self.config
    }

    /// Get the timing metrics recorded during loading, if available.
    pub(crate) fn phase_timing(&self) -> Option<ConfigPhaseTiming> {
        self.phase_timing
    }

    /// Get the configuration file path (if loaded from file)
    pub fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }

    /// Get the active workspace root for this manager.
    pub fn workspace_root(&self) -> Option<&Path> {
        self.workspace_root.as_deref()
    }

    /// Get the config filename used by this manager (usually `vtcode.toml`).
    pub fn config_file_name(&self) -> &str {
        &self.config_file_name
    }

    /// Get the configuration layer stack
    pub fn layer_stack(&self) -> &ConfigLayerStack {
        &self.layer_stack
    }

    /// Resolve the canonical user-level config file VT Code should write to.
    pub fn preferred_user_config_path(&self) -> Option<PathBuf> {
        self.canonical_user_config_path.clone()
    }

    /// Return every supported user-level config path for the loaded config
    /// filename, including paths that do not exist yet.
    ///
    /// Callers that monitor configuration must retain the nonexistent paths:
    /// a later `None -> Some(mtime)` transition is a real configuration change.
    pub fn user_config_paths(&self) -> Vec<PathBuf> {
        let mut paths = self.tracked_user_config_paths.clone();

        for path in self
            .layer_stack
            .layers()
            .iter()
            .filter_map(|layer| match (&layer.source, layer.is_enabled()) {
                (ConfigLayerSource::User { file }, true) => Some(file),
                _ => None,
            })
        {
            if !paths.iter().any(|existing| existing == path) {
                paths.push(path.clone());
            }
        }

        paths
    }

    /// Get the effective TOML configuration
    pub fn effective_config(&self) -> toml::Value {
        self.layer_stack.effective_config()
    }

    /// Get session duration from agent config
    pub fn session_duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs(60 * 60) // Default 1 hour
    }

    /// Persist configuration to a specific path, preserving comments
    pub fn save_config_to_path(path: impl AsRef<Path>, config: &VTCodeConfig) -> Result<()> {
        let path = path.as_ref();
        ensure_private_parent_dir(path)?;
        let sparse_value = Self::sparse_config_value(config).context("Failed to prepare sparse configuration")?;
        let sparse_content =
            toml::to_string_pretty(&sparse_value).context("Failed to serialize sparse configuration")?;

        // If file exists, preserve comments by using toml_edit. Read and
        // publish through the shared no-follow/atomic file policy so a
        // user-controlled symlink cannot redirect configuration writes.
        let existing_content = match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("Refusing to read symlinked config file: {}", path.display())
            }
            Ok(metadata) if !metadata.is_file() => {
                bail!("Config path is not a regular file: {}", path.display())
            }
            Ok(_) => Some(
                String::from_utf8(VtCodePaths::read_file_no_follow(path)?)
                    .with_context(|| format!("Failed to read existing config: {}", path.display()))?,
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to inspect config: {}", path.display()));
            }
        };

        if let Some(original_content) = existing_content {
            let mut doc = original_content
                .parse::<toml_edit::DocumentMut>()
                .with_context(|| format!("Failed to parse existing config: {}", path.display()))?;
            Self::remove_deprecated_config_keys(&mut doc);

            let new_doc: toml_edit::DocumentMut = sparse_content
                .parse()
                .context("Failed to parse sparse serialized configuration")?;
            let default_value =
                toml::Value::try_from(VTCodeConfig::default()).context("Failed to serialize default configuration")?;
            let default_doc: toml_edit::DocumentMut = toml::to_string_pretty(&default_value)
                .context("Failed to serialize default configuration")?
                .parse()
                .context("Failed to parse default serialized configuration")?;

            // Update values while preserving structure and comments
            Self::merge_sparse_toml_documents(&mut doc, &new_doc, &default_doc);

            VtCodePaths::write_private_file_atomic(path, doc.to_string().as_bytes())
                .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        } else {
            VtCodePaths::write_private_file_atomic(path, sparse_content.as_bytes())
                .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        }

        Ok(())
    }

    fn remove_deprecated_config_keys(doc: &mut toml_edit::DocumentMut) {
        let table = doc.as_table_mut();
        table.remove("project_doc_max_bytes");
        table.remove("project_doc_fallback_filenames");
        Self::remove_table_keys(table, "agent", &["autonomous_mode", "default_editing_mode"]);
        Self::remove_table_keys(table, "permissions", &["allowed_tools", "disallowed_tools", "auto_permission"]);
    }

    fn remove_table_keys(table: &mut toml_edit::Table, section: &str, keys: &[&str]) {
        let Some(section) = table.get_mut(section).and_then(toml_edit::Item::as_table_mut) else {
            return;
        };

        for key in keys {
            section.remove(key);
        }
    }

    pub fn sparse_config_value(config: &VTCodeConfig) -> Result<toml::Value> {
        let mut value = toml::Value::try_from(config).context("Failed to serialize configuration")?;
        let default_value =
            toml::Value::try_from(VTCodeConfig::default()).context("Failed to serialize default configuration")?;
        Self::prune_default_values(&mut value, &default_value);
        Ok(value)
    }

    fn prune_default_values(value: &mut toml::Value, default_value: &toml::Value) -> bool {
        match (value, default_value) {
            (toml::Value::Table(table), toml::Value::Table(default_table)) => {
                table.retain(|key, child| {
                    default_table
                        .get(key)
                        .is_none_or(|default_child| !Self::prune_default_values(child, default_child))
                });
                table.is_empty()
            }
            (value, default_value) => value == default_value,
        }
    }

    /// Merge TOML documents, preserving comments and structure from original
    fn merge_sparse_toml_documents(
        original: &mut toml_edit::DocumentMut,
        new: &toml_edit::DocumentMut,
        default_doc: &toml_edit::DocumentMut,
    ) {
        Self::merge_sparse_tables(original.as_table_mut(), new.as_table(), default_doc.as_table());
    }

    fn merge_sparse_tables(original: &mut toml_edit::Table, new: &toml_edit::Table, default_table: &toml_edit::Table) {
        let mut remove_keys = Vec::with_capacity(default_table.len());

        for (key, default_value) in default_table.iter() {
            if let Some(new_value) = new.get(key) {
                if let Some(original_value) = original.get_mut(key) {
                    Self::merge_sparse_items(original_value, new_value, default_value);
                } else {
                    original[key] = new_value.clone();
                }
            } else {
                let Some(original_value) = original.get_mut(key) else {
                    continue;
                };
                if Self::remove_known_default_item(original_value, default_value) {
                    remove_keys.push(key.to_string());
                }
            }
        }

        for key in remove_keys {
            original.remove(&key);
        }

        for (key, new_value) in new.iter() {
            if default_table.contains_key(key) {
                continue;
            }
            if let Some(original_value) = original.get_mut(key) {
                *original_value = new_value.clone();
            } else {
                original[key] = new_value.clone();
            }
        }
    }

    fn merge_sparse_items(original: &mut toml_edit::Item, new: &toml_edit::Item, default_value: &toml_edit::Item) {
        match (original, new, default_value) {
            (
                toml_edit::Item::Table(orig_table),
                toml_edit::Item::Table(new_table),
                toml_edit::Item::Table(default_table),
            ) => Self::merge_sparse_tables(orig_table, new_table, default_table),
            (orig, new, _) => {
                *orig = new.clone();
            }
        }
    }

    fn remove_known_default_item(original: &mut toml_edit::Item, default_value: &toml_edit::Item) -> bool {
        match (original, default_value) {
            (toml_edit::Item::Table(orig_table), toml_edit::Item::Table(default_table)) => {
                let mut remove_keys = Vec::new();
                for (key, default_child) in default_table.iter() {
                    let Some(orig_child) = orig_table.get_mut(key) else {
                        continue;
                    };
                    if Self::remove_known_default_item(orig_child, default_child) {
                        remove_keys.push(key.to_string());
                    }
                }
                for key in remove_keys {
                    orig_table.remove(&key);
                }
                orig_table.is_empty()
            }
            _ => true,
        }
    }

    fn project_config_path(config_dir: &Path, workspace_root: &Path, config_file_name: &str) -> Option<PathBuf> {
        let project_name = Self::identify_current_project(workspace_root)?;
        Some(
            config_dir
                .join("projects")
                .join(project_name)
                .join("config")
                .join(config_file_name),
        )
    }

    fn identify_current_project(workspace_root: &Path) -> Option<String> {
        let project_file = workspace_root.join(".vtcode-project");
        if let Ok(contents) = fs::read_to_string(&project_file) {
            let name = contents.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }

        workspace_root
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string())
    }

    /// Resolve the current project name used for project-level config overlays.
    pub fn current_project_name(workspace_root: &Path) -> Option<String> {
        Self::identify_current_project(workspace_root)
    }

    fn validate_restricted_agent_fields(
        layer_stack: &ConfigLayerStack,
        origins: &hashbrown::HashMap<String, ConfigLayerMetadata>,
    ) -> Result<()> {
        if let Some(origin) = origins.get("agent.persistent_memory.directory_override")
            && let Some(layer) = layer_stack.layers().iter().find(|layer| layer.metadata == *origin)
        {
            match layer.source {
                ConfigLayerSource::System { .. }
                | ConfigLayerSource::User { .. }
                | ConfigLayerSource::Project { .. } => {}
                ConfigLayerSource::Workspace { .. } | ConfigLayerSource::Runtime => {
                    bail!(
                        "agent.persistent_memory.directory_override may only be set in system, user, or project-profile configuration layers"
                    );
                }
            }
        }

        Ok(())
    }

    /// Collect lifecycle hook commands whose effective origin is a
    /// workspace-controlled layer: the workspace-root `vtcode.toml`, the
    /// workspace `.vtcode/vtcode.toml` fallback, or a project profile stored
    /// inside the workspace. These are attacker-influenceable in an untrusted
    /// repository and must be gated behind explicit user approval.
    pub(crate) fn collect_workspace_lifecycle_hooks(
        layer_stack: &ConfigLayerStack,
        hooks: &HooksConfig,
    ) -> WorkspaceLifecycleHooks {
        let (_, origins) = layer_stack.effective_config_with_origins();
        let mut commands = Vec::new();

        for event in crate::hooks::LIFECYCLE_HOOK_EVENTS {
            let origin_key = format!("hooks.lifecycle.{event}");
            let Some(origin) = origins.get(&origin_key) else {
                continue;
            };
            let workspace_controlled = layer_stack.layers().iter().any(|layer| {
                layer.is_enabled()
                    && layer.metadata == *origin
                    && matches!(layer.source, ConfigLayerSource::Workspace { .. } | ConfigLayerSource::Project { .. })
            });
            if !workspace_controlled {
                continue;
            }

            // Deprecated aliases fold into `stop` at engine normalization.
            let tag = match *event {
                "task_completion" | "task_completed" => "stop",
                other => other,
            };
            for group in hooks.lifecycle.groups_for_event(event) {
                for command in &group.hooks {
                    commands.push(WorkspaceHookCommand {
                        event: tag.to_string(),
                        matcher: group.matcher.clone(),
                        command: command.command.clone(),
                        timeout_seconds: command.timeout_seconds,
                    });
                }
            }
        }

        WorkspaceLifecycleHooks { commands }
    }

    /// Persist configuration to the manager's associated path or workspace
    pub fn save_config(&mut self, config: &VTCodeConfig) -> Result<()> {
        if let Some(path) = &self.config_path {
            let target = if self.write_global_config_to_canonical || self.is_global_config_path(path) {
                self.preferred_user_config_path().unwrap_or_else(|| path.clone())
            } else {
                path.clone()
            };
            Self::save_config_to_path(target, config)?;
        } else if let Some(workspace_root) = &self.workspace_root {
            let path = workspace_root.join(&self.config_file_name);
            Self::save_config_to_path(path, config)?;
        } else {
            let cwd = std::env::current_dir().context("Failed to resolve current directory")?;
            let path = cwd.join(&self.config_file_name);
            Self::save_config_to_path(path, config)?;
        }

        #[cfg(not(test))]
        if let Some(workspace) = &self.workspace_root {
            Self::invalidate_workspace_cache(workspace);
        }

        self.sync_from_config(config)
    }

    fn is_global_config_path(&self, path: &Path) -> bool {
        self.layer_stack.layers().iter().any(|layer| {
            layer.is_enabled()
                && matches!(layer.source, ConfigLayerSource::System { .. } | ConfigLayerSource::User { .. })
                && match &layer.source {
                    ConfigLayerSource::System { file } | ConfigLayerSource::User { file } => file == path,
                    _ => false,
                }
        })
    }

    /// Sync internal config from a saved config
    /// Call this after save_config to keep internal state in sync
    fn sync_from_config(&mut self, config: &VTCodeConfig) -> Result<()> {
        self.config = config.clone();
        Ok(())
    }
}

/// Migrate plain-text API keys from config to secure storage.
///
/// This function checks if there are any API keys stored in plain-text in the config
/// and migrates them to secure storage (keyring). After successful migration,
/// the key material is cleared and a normalized provider/key metadata marker is
/// retained for configuration introspection.
///
/// # Arguments
/// * `config` - The configuration to migrate
fn migrate_custom_api_keys_if_needed(config: &mut VTCodeConfig) -> Result<()> {
    let storage_mode = config.agent.credential_storage_mode;

    // Check if there are any non-empty API keys in the config
    let has_plain_text_keys = config.agent.custom_api_keys.values().any(|key| !key.is_empty());

    if has_plain_text_keys {
        tracing::info!("Detected plain-text API keys in config, migrating to secure storage...");

        let mut migrated_count = 0;
        let pending = config
            .agent
            .custom_api_keys
            .iter()
            .filter(|(_, key)| !key.is_empty())
            .map(|(provider, key)| (provider.clone(), key.clone()))
            .collect::<Vec<_>>();
        for (provider, api_key) in pending {
            let key_name = config
                .configured_api_key_env(&provider)
                .unwrap_or_else(|| api_key_env_var(&provider));
            if let Some(identity) = store_credential_with_mode(&provider, &key_name, &api_key, storage_mode)? {
                config.agent.custom_api_keys.remove(&provider);
                if let Some(metadata_key) = credential_metadata_key(identity.provider(), identity.key_name())? {
                    config.agent.custom_api_keys.insert(metadata_key, String::new());
                }
                migrated_count += 1;
            }
        }

        if migrated_count > 0 {
            tracing::info!("Successfully migrated {} API key(s) to secure storage", migrated_count);
            tracing::warn!(
                "Plain-text API keys have been cleared from config file. \
                 Please commit the updated config to remove sensitive data from version control."
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod sparse_config_tests {
    use super::*;

    #[test]
    fn sparse_config_omits_default_primary_agent_when_unmodified() {
        let config = VTCodeConfig::default();
        let value = ConfigManager::sparse_config_value(&config).expect("sparse config");

        assert!(value.get("default_primary_agent").is_none());
    }

    #[test]
    fn sparse_config_persists_non_default_primary_agent() {
        let config = VTCodeConfig {
            default_primary_agent: "auto".to_string(),
            ..Default::default()
        };

        let value = ConfigManager::sparse_config_value(&config).expect("sparse config");

        assert_eq!(value.get("default_primary_agent").and_then(toml::Value::as_str), Some("auto"));
    }

    #[test]
    fn save_config_migrates_legacy_permissions_auto_permission_table() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let config_path = temp_dir.path().join("vtcode.toml");
        fs::write(
            &config_path,
            r#"
[permissions.auto_permission]
model = "legacy-reviewer"
max_consecutive_denials = 2
"#,
        )
        .expect("write legacy config");

        let mut config = VTCodeConfig::default();
        config.permissions.auto_permission.model = "legacy-reviewer".to_string();
        config.permissions.auto_permission.max_consecutive_denials = 2;

        ConfigManager::save_config_to_path(&config_path, &config).expect("save config");

        let saved_content = fs::read_to_string(&config_path).expect("read saved config");
        assert!(
            saved_content.contains("[permissions.auto]"),
            "canonical auto permission table should be persisted. Got:\n{saved_content}"
        );
        assert!(
            !saved_content.contains("auto_permission"),
            "legacy auto_permission table should be removed. Got:\n{saved_content}"
        );

        let reloaded = ConfigManager::load_from_file(&config_path).expect("reload saved config");
        assert_eq!(reloaded.config().permissions.auto_permission.model, "legacy-reviewer");
        assert_eq!(reloaded.config().permissions.auto_permission.max_consecutive_denials, 2);
    }

    #[test]
    fn save_config_removes_legacy_permissions_auto_permission_when_sparse_default() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let config_path = temp_dir.path().join("vtcode.toml");
        fs::write(
            &config_path,
            r#"
[permissions.auto_permission]
max_consecutive_denials = 3
"#,
        )
        .expect("write legacy config");

        ConfigManager::save_config_to_path(&config_path, &VTCodeConfig::default()).expect("save config");

        let saved_content = fs::read_to_string(&config_path).expect("read saved config");
        assert!(
            !saved_content.contains("auto_permission"),
            "legacy auto_permission table should be removed. Got:\n{saved_content}"
        );
        assert!(
            !saved_content.contains("[permissions.auto]"),
            "default auto permission config should remain sparse. Got:\n{saved_content}"
        );

        ConfigManager::load_from_file(&config_path).expect("reload saved config");
    }

    #[test]
    fn inaccessible_optional_global_layers_are_skipped() {
        let error = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(should_skip_optional_global_error(
            &ConfigLayerSource::System { file: PathBuf::from("/etc/vtcode/vtcode.toml") },
            &error
        ));
        assert!(should_skip_optional_global_error(
            &ConfigLayerSource::User {
                file: PathBuf::from("/home/user/.config/vtcode/vtcode.toml")
            },
            &error
        ));
        assert!(!should_skip_optional_global_error(
            &ConfigLayerSource::Workspace { file: PathBuf::from("/workspace/vtcode.toml") },
            &error
        ));
    }
}

#[cfg(test)]
mod workspace_lifecycle_hooks_tests {
    use super::*;
    use crate::hooks::WorkspaceLifecycleHooks;

    fn layer(source: ConfigLayerSource, content: &str) -> ConfigLayerEntry {
        ConfigLayerEntry::new(source, toml::from_str(content).expect("valid test toml"))
    }

    fn collect(stack: &ConfigLayerStack) -> WorkspaceLifecycleHooks {
        let (effective_toml, _) = stack.effective_config_with_origins();
        let config: VTCodeConfig = effective_toml.try_into().expect("effective config parses");
        ConfigManager::collect_workspace_lifecycle_hooks(stack, &config.hooks)
    }

    const WS_SESSION_START: &str = r#"
[[hooks.lifecycle.session_start]]
matcher = "startup"

[[hooks.lifecycle.session_start.hooks]]
command = "echo workspace-session"
timeout_seconds = 30
"#;

    const USER_SESSION_END: &str = r#"
[[hooks.lifecycle.session_end]]
[[hooks.lifecycle.session_end.hooks]]
command = "echo user-session-end"
"#;

    #[test]
    fn workspace_layer_hooks_are_collected_user_hooks_are_not() {
        let mut stack = ConfigLayerStack::default();
        stack.push(layer(ConfigLayerSource::User { file: "/home/u/.vtcode/vtcode.toml".into() }, USER_SESSION_END));
        stack.push(layer(ConfigLayerSource::Workspace { file: "/ws/vtcode.toml".into() }, WS_SESSION_START));

        let collected = collect(&stack);

        assert_eq!(collected.commands.len(), 1, "only workspace hooks should be collected");
        let command = &collected.commands[0];
        assert_eq!(command.event, "session_start");
        assert_eq!(command.command, "echo workspace-session");
        assert_eq!(command.matcher.as_deref(), Some("startup"));
        assert_eq!(command.timeout_seconds, Some(30));
        assert!(!collected.is_empty());
    }

    #[test]
    fn user_only_hooks_are_not_workspace_controlled() {
        let mut stack = ConfigLayerStack::default();
        stack.push(layer(ConfigLayerSource::User { file: "/home/u/.vtcode/vtcode.toml".into() }, USER_SESSION_END));

        let collected = collect(&stack);

        assert!(collected.is_empty());
    }

    #[test]
    fn deprecated_task_completion_aliases_fold_into_stop() {
        let mut stack = ConfigLayerStack::default();
        stack.push(layer(
            ConfigLayerSource::Workspace { file: "/ws/vtcode.toml".into() },
            r#"
[[hooks.lifecycle.task_completion]]
[[hooks.lifecycle.task_completion.hooks]]
command = "echo ws-task-completion"
"#,
        ));

        let collected = collect(&stack);

        assert_eq!(collected.commands.len(), 1);
        assert_eq!(collected.commands[0].event, "stop");
        assert_eq!(collected.commands[0].command, "echo ws-task-completion");
    }

    #[test]
    fn empty_workspace_hook_arrays_do_not_flag_content() {
        let mut stack = ConfigLayerStack::default();
        stack.push(layer(
            ConfigLayerSource::Workspace { file: "/ws/vtcode.toml".into() },
            "[hooks.lifecycle]\nsession_start = []\n",
        ));

        let collected = collect(&stack);

        assert!(collected.is_empty(), "an empty workspace hook array is not executable content");
    }

    #[test]
    fn load_from_file_populates_workspace_lifecycle_hooks() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let config_path = temp_dir.path().join("vtcode.toml");
        fs::write(&config_path, WS_SESSION_START).expect("write workspace config");

        let manager = ConfigManager::load_from_file(&config_path).expect("load config");
        let workspace_hooks = manager
            .config()
            .workspace_lifecycle_hooks
            .as_ref()
            .expect("workspace hooks populated at load");

        assert!(!workspace_hooks.is_empty());
        assert_eq!(workspace_hooks.commands.len(), 1);
        assert_eq!(workspace_hooks.commands[0].command, "echo workspace-session");
    }
}
