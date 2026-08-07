use std::path::{Path, PathBuf};

pub fn expand_placeholders(value: &str, plugin_root: &Path, plugin_data: &Path) -> String {
    value
        .replace("${PLUGIN_ROOT}", &plugin_root.display().to_string())
        .replace("${PLUGIN_DATA}", &plugin_data.display().to_string())
}

#[allow(dead_code)]
pub fn is_escaped_from_root(candidate: &Path, root: &Path) -> bool {
    !candidate.starts_with(root)
}

pub fn validate_plugin_relative(value: &str, root: &Path) -> Result<PathBuf, crate::PluginError> {
    if !value.starts_with("./") {
        return Err(crate::PluginError::PathEscape(format!("path must start with ./: {}", value)));
    }

    let candidate = root.join(&value[2..]);
    let Ok(candidate) = std::path::absolute(&candidate) else {
        return Err(crate::PluginError::PathEscape(format!("path could not be resolved: {}", value)));
    };

    let Ok(root) = std::path::absolute(root) else {
        return Err(crate::PluginError::PathEscape("plugin root could not be resolved".into()));
    };

    if !candidate.starts_with(&root) {
        return Err(crate::PluginError::PathEscape(format!("path escapes plugin root: {}", value)));
    }

    Ok(candidate)
}
