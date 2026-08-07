use std::path::{Path, PathBuf};

use crate::errors::PluginError;
use crate::manifest::PluginManifest;
use crate::mcp::McpConfig;
use vtcode_skills::types::SkillManifest;

#[derive(Debug, Clone)]
pub struct DiscoveredSkill {
    pub name: String,
    pub dir_name: String,
    pub skill_md_path: PathBuf,
    pub manifest: SkillManifest,
}

#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub root: PathBuf,
    pub skills: Vec<DiscoveredSkill>,
    pub mcp: Option<McpConfig>,
}

impl LoadedPlugin {
    pub fn load_from_dir(plugin_root: &Path) -> Result<Self, PluginError> {
        let root = std::path::absolute(plugin_root).map_err(PluginError::Io)?;

        let manifest_path = root.join("plugin.json");
        let content = std::fs::read_to_string(&manifest_path)?;
        let (manifest, unknown_fields) = PluginManifest::parse(&content)?;
        for field in &unknown_fields {
            tracing::warn!(field = field, "unknown top-level field in plugin.json");
        }

        let skills = discover_skills(&root)?;
        let mut mcp = discover_mcp(&root)?;

        if let Some(ref mcp_config) = mcp {
            if !mcp_config.schema.is_empty()
                && !manifest.schema.is_empty()
                && extract_schema_version(&manifest.schema) != extract_schema_version(&mcp_config.schema)
            {
                tracing::warn!(
                    manifest_version = ?extract_schema_version(&manifest.schema),
                    mcp_version = ?extract_schema_version(&mcp_config.schema),
                    "mcp.json schema version does not match plugin.json; disabling MCP for plugin"
                );
                mcp = None;
            }
        }

        Ok(LoadedPlugin { manifest, root, skills, mcp })
    }
}

fn extract_schema_version(schema: &str) -> Option<&str> {
    schema
        .strip_prefix("https://agent-plugins.org/schemas/")
        .and_then(|rest| rest.split('/').next())
}

fn discover_skills(plugin_root: &Path) -> Result<Vec<DiscoveredSkill>, PluginError> {
    let skills_dir = plugin_root.join("skills");
    if !skills_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut skills = Vec::new();
    for entry in std::fs::read_dir(&skills_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }

        let content = std::fs::read_to_string(&skill_md)?;
        let (skill_manifest, _instructions) = vtcode_skills::manifest::parse_skill_content(&content)
            .map_err(|e| PluginError::InvalidSkill(e.to_string()))?;

        let dir_name = path
            .file_name()
            .and_then(|s| s.to_string_lossy().into_owned().into())
            .ok_or_else(|| PluginError::InvalidSkill("invalid skill directory name".into()))?;

        if skill_manifest.name != dir_name {
            return Err(PluginError::InvalidSkill(format!(
                "skill name '{}' does not match directory '{}'",
                skill_manifest.name, dir_name
            )));
        }

        skill_manifest
            .validate()
            .map_err(|e| PluginError::InvalidSkill(e.to_string()))?;

        let manifest = skill_manifest.clone();
        skills.push(DiscoveredSkill {
            name: skill_manifest.name,
            dir_name: dir_name.to_string(),
            skill_md_path: skill_md,
            manifest,
        });
    }

    Ok(skills)
}

fn discover_mcp(plugin_root: &Path) -> Result<Option<McpConfig>, PluginError> {
    let mcp_path = plugin_root.join("mcp.json");
    if !mcp_path.is_file() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&mcp_path)?;
    let config = McpConfig::parse(&content)?;
    if !config.unknown_fields.is_empty() {
        tracing::warn!(
            fields = ?config.unknown_fields,
            "unknown top-level fields in plugin mcp.json; disabling MCP for this plugin"
        );
        return Ok(None);
    }
    Ok(Some(config))
}
