use std::path::Path;

use crate::discovery::LoadedPlugin;
use crate::errors::PluginError;

/// Abstraction for loading Agent Plugins from a source.
///
/// This trait isolates the loading mechanism from the rest of the system,
/// allowing the next generation to swap the filesystem implementation for
/// network-loaded, cached, or sandboxed loaders without changing callers.
pub trait PluginLoader {
    /// Load a plugin from the given path.
    fn load(&self, path: &Path) -> Result<LoadedPlugin, PluginError>;
}

/// Default filesystem-based plugin loader.
///
/// This is the current production implementation. It reads `plugin.json`,
/// `skills/`, and `mcp.json` from the local filesystem.
pub struct FileSystemPluginLoader {
    _private: (),
}

impl FileSystemPluginLoader {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for FileSystemPluginLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginLoader for FileSystemPluginLoader {
    fn load(&self, path: &Path) -> Result<LoadedPlugin, PluginError> {
        LoadedPlugin::load_from_dir(path)
    }
}

/// Abstraction for validating loaded plugins.
///
/// Callers should validate plugins after loading and before use.
/// The default implementation accepts all valid plugins.
pub trait PluginValidator {
    /// Validate a loaded plugin. Returns `Ok(())` if the plugin is acceptable.
    fn validate(&self, plugin: &LoadedPlugin) -> Result<(), PluginError>;
}

/// Default validator that accepts all successfully-loaded plugins.
///
/// Additional checks (trust model, signature, etc.) can be layered by
/// wrapping this validator with a decorator.
pub struct DefaultPluginValidator;

impl PluginValidator for DefaultPluginValidator {
    fn validate(&self, _plugin: &LoadedPlugin) -> Result<(), PluginError> {
        Ok(())
    }
}

/// Outcome of a plugin installation.
pub struct InstalledPlugin {
    pub name: String,
    pub path: std::path::PathBuf,
    pub loaded: LoadedPlugin,
}

/// Abstraction for installing and removing plugins.
///
/// This isolates installation mechanics (git clone, local copy, etc.)
/// from the CLI layer, enabling test doubles and alternate install targets.
pub trait PluginInstaller {
    /// Install a plugin from `source` with the given `name`.
    fn install(&self, source: &str, name: Option<String>) -> Result<InstalledPlugin, PluginError>;

    /// Remove an installed plugin by `name`.
    fn remove(&self, name: &str) -> Result<(), PluginError>;
}

/// Default filesystem-based installer.
pub struct FileSystemPluginInstaller {
    _private: (),
}

impl FileSystemPluginInstaller {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for FileSystemPluginInstaller {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginInstaller for FileSystemPluginInstaller {
    fn install(&self, source: &str, name: Option<String>) -> Result<InstalledPlugin, PluginError> {
        let target_name = name.unwrap_or_else(|| {
            Path::new(source)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("plugin")
                .to_string()
        });

        let home = dirs::home_dir().ok_or_else(|| {
            PluginError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "cannot resolve home directory"))
        })?;
        let install_dir = home.join(".agents/plugins").join(&target_name);

        if install_dir.exists() {
            return Err(PluginError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("plugin already installed at {}", install_dir.display()),
            )));
        }

        if source.starts_with("https://")
            || source.starts_with("http://")
            || source.starts_with("git@")
            || source.starts_with("ssh://")
            || source.ends_with(".git")
        {
            let status = std::process::Command::new("git")
                .args(["clone", "--depth=1", source, &install_dir.to_string_lossy()])
                .status()
                .map_err(PluginError::Io)?;
            if !status.success() {
                return Err(PluginError::Io(std::io::Error::other(format!("git clone failed for {}", source))));
            }
        } else {
            let src = Path::new(source);
            if !src.is_dir() {
                return Err(PluginError::InvalidManifest(format!("source is not a directory or git URL: {}", source)));
            }
            if !src.join("plugin.json").is_file() {
                return Err(PluginError::InvalidManifest(format!(
                    "source directory does not contain a valid plugin.json: {}",
                    source
                )));
            }
            std::fs::create_dir_all(&install_dir).map_err(PluginError::Io)?;
            copy_dir_all(src, &install_dir).map_err(PluginError::Io)?;
        }

        let loaded = LoadedPlugin::load_from_dir(&install_dir)?;
        Ok(InstalledPlugin { name: target_name, path: install_dir, loaded })
    }

    fn remove(&self, name: &str) -> Result<(), PluginError> {
        let home = dirs::home_dir().ok_or_else(|| {
            PluginError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "cannot resolve home directory"))
        })?;
        let install_dir = home.join(".agents/plugins").join(name);
        if !install_dir.exists() {
            return Err(PluginError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("plugin not installed: {}", name),
            )));
        }
        if !install_dir.join("plugin.json").is_file() {
            return Err(PluginError::InvalidManifest(format!(
                "not a valid agent plugin (missing plugin.json): {}",
                install_dir.display()
            )));
        }
        std::fs::remove_dir_all(&install_dir).map_err(PluginError::Io)
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
