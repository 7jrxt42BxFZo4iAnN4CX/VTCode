use super::*;
use std::fs::{self, File, OpenOptions};
use std::io;
use vtcode_commons::{VtCodePaths, canonicalize};

pub(crate) fn persistent_memory_base_dir(config: &PersistentMemoryConfig) -> Result<PathBuf> {
    if let Some(override_dir) = config.directory_override.as_deref() {
        if let Some(stripped) = override_dir.strip_prefix("~/") {
            return Ok(dirs::home_dir().context("Could not resolve home directory")?.join(stripped));
        }
        return Ok(PathBuf::from(override_dir));
    }
    VtCodePaths::resolve()
        .map(|paths| paths.state_dir().to_path_buf())
        .context("Could not resolve VT Code state directory")
}

pub(crate) fn persistent_memory_project_name(workspace_root: &Path) -> String {
    ConfigManager::current_project_name(workspace_root)
        .or_else(|| workspace_root.file_name().and_then(|v| v.to_str()).map(|v| v.to_string()))
        .unwrap_or_else(|| "workspace".to_string())
}

pub(crate) fn migrate_legacy_persistent_memory_dir_if_needed(
    config: &PersistentMemoryConfig,
    project_name: &str,
    target_dir: &Path,
) -> Result<()> {
    if config.directory_override.is_some() {
        return Ok(());
    }
    let Some(legacy_dir) = legacy_persistent_memory_dir(project_name)? else {
        return Ok(());
    };
    if legacy_dir == target_dir || !legacy_dir.exists() {
        return Ok(());
    }
    migrate_legacy_memory_dir(&legacy_dir, target_dir)
}

pub(super) fn migrate_legacy_memory_dir(legacy_dir: &Path, target_dir: &Path) -> Result<()> {
    validate_no_escaping_symlink_ancestors(legacy_dir, false)
        .with_context(|| format!("Refusing to migrate memory from unsafe source {}", legacy_dir.display()))?;
    validate_no_escaping_symlink_ancestors(target_dir, true)
        .with_context(|| format!("Refusing to migrate memory into unsafe target {}", target_dir.display()))?;
    if target_dir.exists() && memory_directory_has_stored_content(target_dir)? {
        return Ok(());
    }
    if target_dir.exists() {
        fs::remove_dir_all(target_dir).with_context(|| format!("Failed to clear {}", target_dir.display()))?;
    }
    let target_parent = target_dir.parent().context("Persistent memory directory is missing a parent")?;
    VtCodePaths::ensure_user_dir(target_parent)
        .with_context(|| format!("Failed to create {}", target_parent.display()))?;
    copy_memory_tree(legacy_dir, target_dir)?;
    Ok(())
}

fn validate_no_escaping_symlink_ancestors(path: &Path, allow_missing_leaf: bool) -> Result<()> {
    let components = path.components().collect::<Vec<_>>();
    let mut current = PathBuf::new();
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        let is_leaf = index + 1 == components.len();
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if allow_missing_leaf && error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error).with_context(|| format!("could not inspect {}", current.display())),
        };
        let is_directory = if metadata.file_type().is_symlink() {
            if is_leaf {
                bail!("final path component {} is a symlink", current.display());
            }
            let parent = current.parent().context("symlink path component is missing its parent")?;
            let canonical_parent = canonicalize(parent)
                .with_context(|| format!("could not resolve symlink parent {}", parent.display()))?;
            let canonical_target =
                canonicalize(&current).with_context(|| format!("could not resolve symlink {}", current.display()))?;
            if !canonical_target.starts_with(&canonical_parent) {
                bail!("path component {} is a symlink outside its containing directory", current.display());
            }
            fs::metadata(&current)
                .with_context(|| format!("could not inspect symlink target {}", current.display()))?
                .is_dir()
        } else {
            metadata.is_dir()
        };
        if !is_leaf && !is_directory {
            bail!("path component {} is not a directory", current.display());
        }
    }
    Ok(())
}

fn copy_memory_tree(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("Failed to inspect legacy memory path {}", source.display()))?;
    if metadata.file_type().is_symlink() {
        bail!("Refusing to migrate symlinked memory path {}", source.display());
    }
    if metadata.is_dir() {
        VtCodePaths::ensure_user_dir(destination)
            .with_context(|| format!("Failed to create memory directory {}", destination.display()))?;
        for entry in fs::read_dir(source).with_context(|| format!("Failed to list {}", source.display()))? {
            let entry = entry?;
            copy_memory_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if !metadata.is_file() {
        bail!("Refusing to migrate special memory path {}", source.display());
    }
    if let Ok(destination_metadata) = fs::symlink_metadata(destination) {
        if destination_metadata.file_type().is_symlink() {
            bail!("Refusing to migrate memory through symlink {}", destination.display());
        }
        if destination_metadata.is_file() {
            return Ok(());
        }
        bail!("Refusing to replace non-regular memory destination {}", destination.display());
    }
    let parent = destination.parent().context("Memory destination is missing a parent")?;
    VtCodePaths::ensure_user_dir(parent)
        .with_context(|| format!("Failed to create memory directory {}", parent.display()))?;

    let temporary = parent.join(format!(
        ".{}.{}.memory-migration",
        destination.file_name().and_then(|name| name.to_str()).unwrap_or("file"),
        std::process::id()
    ));
    let mut input = open_no_follow(source)
        .with_context(|| format!("Failed to safely open legacy memory file {}", source.display()))?;
    let mut output = create_private_new_file(&temporary)
        .with_context(|| format!("Failed to create temporary memory file {}", temporary.display()))?;
    io::copy(&mut input, &mut output).with_context(|| format!("Failed to copy memory file {}", source.display()))?;
    output
        .sync_all()
        .with_context(|| format!("Failed to sync temporary memory file {}", temporary.display()))?;
    drop(output);

    match fs::hard_link(&temporary, destination) {
        Ok(()) => {
            fs::remove_file(&temporary)
                .with_context(|| format!("Failed to remove temporary memory file {}", temporary.display()))?;
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&temporary);
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            return Err(error).with_context(|| format!("Failed to publish memory file {}", destination.display()));
        }
    }
    Ok(())
}

fn create_private_new_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

fn open_no_follow(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

fn legacy_persistent_memory_dir(project_name: &str) -> Result<Option<PathBuf>> {
    let paths = VtCodePaths::resolve().context("Could not resolve VT Code paths")?;
    let legacy_base = paths.legacy_dir().to_path_buf();
    let current_base = paths.state_dir().to_path_buf();
    if legacy_base == current_base {
        return Ok(None);
    }
    Ok(Some(
        legacy_base
            .join("projects")
            .join(sanitize_project_name(project_name))
            .join("memory"),
    ))
}

fn memory_directory_has_stored_content(directory: &Path) -> Result<bool> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).with_context(|| format!("Failed to inspect {}", directory.display())),
    };
    if metadata.file_type().is_symlink() {
        bail!("Refusing to inspect symlinked memory directory {}", directory.display());
    }
    if !metadata.is_dir() {
        bail!("Persistent memory path {} is not a directory", directory.display());
    }

    for path in [
        directory.join(PREFERENCES_FILENAME),
        directory.join(REPOSITORY_FACTS_FILENAME),
    ] {
        if let Some(contents) = read_regular_memory_file(&path)?
            && !parse_topic_file(&contents).is_empty()
        {
            return Ok(true);
        }
    }

    let rollout_dir = directory.join(ROLLOUT_SUMMARIES_DIRNAME);
    let rollout_metadata = match fs::symlink_metadata(&rollout_dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).with_context(|| format!("Failed to inspect {}", rollout_dir.display())),
    };
    if rollout_metadata.file_type().is_symlink() {
        bail!("Refusing to inspect symlinked rollout directory {}", rollout_dir.display());
    }
    if !rollout_metadata.is_dir() {
        bail!("Persistent memory rollout path {} is not a directory", rollout_dir.display());
    }

    for entry in fs::read_dir(&rollout_dir).with_context(|| format!("Failed to list {}", rollout_dir.display()))? {
        let path = entry?.path();
        if path.extension().and_then(|v| v.to_str()) != Some("md") {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).with_context(|| format!("Failed to inspect {}", path.display()))?;
        if metadata.file_type().is_symlink() {
            bail!("Refusing to inspect symlinked rollout file {}", path.display());
        }
        if !metadata.is_file() {
            bail!("Persistent memory rollout path {} is not a regular file", path.display());
        }
        let bytes = VtCodePaths::read_file_no_follow(&path)
            .with_context(|| format!("Failed to read persistent memory rollout file {}", path.display()))?;
        let contents = String::from_utf8(bytes)
            .with_context(|| format!("Failed to decode persistent memory rollout file {} as UTF-8", path.display()))?;
        if !parse_topic_file(&contents).is_empty() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn read_regular_memory_file(path: &Path) -> Result<Option<String>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("Failed to inspect {}", path.display())),
    };
    if metadata.file_type().is_symlink() {
        bail!("Refusing to inspect symlinked memory file {}", path.display());
    }
    if !metadata.is_file() {
        bail!("Persistent memory path {} is not a regular file", path.display());
    }
    let bytes = VtCodePaths::read_file_no_follow(path)
        .with_context(|| format!("Failed to read persistent memory file {}", path.display()))?;
    String::from_utf8(bytes)
        .map(Some)
        .with_context(|| format!("Failed to decode persistent memory file {} as UTF-8", path.display()))
}

#[cold]
pub(crate) fn sanitize_project_name(project_name: &str) -> String {
    let sanitized: String = project_name
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' => '_',
            other => other,
        })
        .collect();
    let trimmed = sanitized.trim();
    if trimmed.is_empty() {
        "workspace".to_string()
    } else {
        trimmed.to_string()
    }
}
