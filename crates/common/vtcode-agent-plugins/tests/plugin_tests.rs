use std::path::PathBuf;
use vtcode_agent_plugins::*;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(FIXTURES).join(path)
}

#[test]
fn parse_minimal_manifest() {
    let content = r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "test"}"#;
    let (manifest, unknown) = PluginManifest::parse(content).unwrap();
    assert_eq!(manifest.name, "test");
    assert_eq!(manifest.schema, "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json");
    assert!(unknown.is_empty());
}

#[test]
fn parse_full_manifest() {
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
        "name": "example-plugin",
        "version": "1.0.0",
        "description": "An example plugin",
        "author": {"name": "Author", "email": "a@b.com", "url": "https://example.com"},
        "homepage": "https://docs.example.com",
        "repository": "https://github.com/example/plugin",
        "license": "MIT",
        "keywords": ["example", "test"],
        "extensions": {"com.vtcode": {"setting": true}}
    }"#;
    let (manifest, unknown) = PluginManifest::parse(content).unwrap();
    assert_eq!(manifest.name, "example-plugin");
    assert_eq!(manifest.version, Some("1.0.0".into()));
    assert_eq!(manifest.description, Some("An example plugin".into()));
    assert!(manifest.author.is_some());
    assert_eq!(manifest.author.unwrap().name, Some("Author".into()));
    assert!(unknown.is_empty());
}

#[test]
fn reject_invalid_name_uppercase() {
    assert!(PluginManifest::validate_name("My-Plugin").is_err());
}

#[test]
fn reject_invalid_name_leading_hyphen() {
    assert!(PluginManifest::validate_name("-start").is_err());
}

#[test]
fn reject_invalid_name_consecutive_hyphens() {
    assert!(PluginManifest::validate_name("has--double").is_err());
}

#[test]
fn reject_invalid_name_consecutive_periods() {
    assert!(PluginManifest::validate_name("too.many..dots").is_err());
}

#[test]
fn reject_empty_name() {
    assert!(PluginManifest::validate_name("").is_err());
}

#[test]
fn accept_valid_names() {
    assert!(PluginManifest::validate_name("a").is_ok());
    assert!(PluginManifest::validate_name("my-plugin").is_ok());
    assert!(PluginManifest::validate_name("a.b-c.2").is_ok());
}

#[test]
fn reject_missing_required_fields() {
    let content = r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json"}"#;
    assert!(PluginManifest::parse(content).is_err());
}

#[test]
fn reject_unsupported_schema() {
    let content = r#"{"$schema": "https://agent-plugins.org/schemas/9.9.9/plugin.schema.json", "name": "test"}"#;
    assert!(PluginManifest::parse(content).is_err());
}

#[test]
fn reject_wrong_typed_optional_field() {
    // A wrong-typed optional field must fail loudly rather than be silently dropped.
    let content =
        r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "test", "version": 42}"#;
    assert!(PluginManifest::parse(content).is_err());
}

#[test]
fn report_unknown_fields() {
    let content =
        r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "test", "unknown": true}"#;
    let (_, unknown) = PluginManifest::parse(content).unwrap();
    assert_eq!(unknown, vec!["unknown"]);
}

#[test]
fn reject_bad_author_field() {
    let content = r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "bad", "author": {"name": 123}}"#;
    assert!(PluginManifest::parse(content).is_err());
}

#[test]
fn reject_bad_author_unsupported_field() {
    let content = r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "bad", "author": {"name": "Test", "unsupported": true}}"#;
    assert!(PluginManifest::parse(content).is_err());
}

#[test]
fn parse_stdio_mcp_server() {
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "test": {
                "type": "stdio",
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/server-test"],
                "env": {"KEY": "value"},
                "cwd": "./data"
            }
        }
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert_eq!(config.schema, "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json");
    assert!(config.servers.contains_key("test"));
}

#[test]
fn parse_http_mcp_server() {
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "remote": {
                "type": "streamable-http",
                "url": "https://example.com/mcp",
                "headers": {"X-Tenant": "public"}
            }
        }
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert!(config.servers.contains_key("remote"));
}

#[test]
fn parse_sse_mcp_server() {
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "legacy": {
                "type": "sse",
                "url": "https://legacy.example.com/sse"
            }
        }
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert!(config.servers.contains_key("legacy"));
}

#[test]
fn skip_invalid_server_entry() {
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "valid": {"type": "stdio", "command": "echo"},
            "invalid": {"type": "stdio"}
        }
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert!(config.servers.contains_key("valid"));
    assert!(!config.servers.contains_key("invalid"));
}

#[test]
fn report_unknown_mcp_top_level_fields() {
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {},
        "extra": true
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert_eq!(config.unknown_fields, vec!["extra"]);
}

#[test]
fn reject_unsupported_mcp_schema() {
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/9.9.9/mcp.schema.json",
        "mcpServers": {}
    }"#;
    assert!(McpConfig::parse(content).is_err());
}

#[test]
fn reject_wrong_typed_mcp_fields() {
    // Wrong-typed args/env/cwd must fail loudly, not be silently dropped.
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "test": {"type": "stdio", "command": "echo", "args": "not-an-array"}
        }
    }"#;
    assert!(McpConfig::parse(content).is_err());

    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "test": {"type": "stdio", "command": "echo", "args": [1, 2]}
        }
    }"#;
    assert!(McpConfig::parse(content).is_err());

    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "test": {"type": "stdio", "command": "echo", "cwd": 42}
        }
    }"#;
    assert!(McpConfig::parse(content).is_err());
}

#[test]
fn load_example_plugin() {
    let plugin = LoadedPlugin::load_from_dir(&fixture("agent-plugins-example")).unwrap();
    assert_eq!(plugin.manifest.name, "agent-plugins-example");
    assert_eq!(plugin.manifest.version, Some("1.0.0".into()));
    assert_eq!(plugin.skills.len(), 1);
    assert_eq!(plugin.skills[0].name, "migrate-agent-plugin");
    assert_eq!(plugin.skills[0].dir_name, "migrate-agent-plugin");
    assert!(plugin.mcp.is_none());
}

#[test]
fn reject_invalid_name_plugin() {
    let result = LoadedPlugin::load_from_dir(&fixture("invalid-name"));
    assert!(result.is_err());
}

#[test]
fn reject_missing_name_plugin() {
    let result = LoadedPlugin::load_from_dir(&fixture("missing-name"));
    assert!(result.is_err());
}

#[test]
fn reject_bad_author_plugin() {
    let result = LoadedPlugin::load_from_dir(&fixture("bad-author"));
    assert!(result.is_err());
}

#[test]
fn partial_mcp_still_loads_skills() {
    let plugin = LoadedPlugin::load_from_dir(&fixture("partial-mcp")).unwrap();
    // mcp.json has an invalid server entry, but the plugin itself loads
    assert_eq!(plugin.manifest.name, "partial-mcp");
    assert!(plugin.mcp.is_some());
    assert!(plugin.mcp.unwrap().servers.contains_key("valid"));
}

#[test]
fn expand_placeholders_basic() {
    let root = PathBuf::from("/home/user/.agents/plugins/test");
    let data = PathBuf::from("/home/user/.agents/plugins/data/test");
    let expanded = expand_placeholders("${PLUGIN_ROOT}/config", &root, &data);
    assert_eq!(expanded, "/home/user/.agents/plugins/test/config");
}

#[test]
fn expand_placeholders_multiple() {
    let root = PathBuf::from("/home/user/.agents/plugins/test");
    let data = PathBuf::from("/home/user/.agents/plugins/data/test");
    let expanded = expand_placeholders("${PLUGIN_ROOT}/config:${PLUGIN_DATA}/state", &root, &data);
    assert_eq!(expanded, "/home/user/.agents/plugins/test/config:/home/user/.agents/plugins/data/test/state");
}

#[test]
fn validate_plugin_relative_accepts_dot_slash() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::create_dir_all(root.join("bin")).unwrap();
    assert!(validate_plugin_relative("./bin/server", &root).is_ok());
}

#[test]
fn validate_plugin_relative_rejects_bare_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    assert!(validate_plugin_relative("bin/server", &root).is_err());
}

#[test]
fn validate_plugin_relative_rejects_parent_escape() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    assert!(validate_plugin_relative("../bin/server", &root).is_err());
}

#[cfg(unix)]
#[test]
fn validate_plugin_relative_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("plugin");
    std::fs::create_dir_all(&root).unwrap();
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    symlink(&outside, root.join("link")).unwrap();
    assert!(validate_plugin_relative("./link", &root).is_err());
}

#[test]
fn validate_plugin_relative_accepts_nonexistent_leaf() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    // A leaf that does not exist yet (e.g. a binary built at runtime) is fine
    // as long as its existing ancestors stay inside the root.
    std::fs::create_dir_all(root.join("bin")).unwrap();
    assert!(validate_plugin_relative("./bin/not-yet-built", &root).is_ok());
}

#[test]
fn install_refuses_duplicate_plugin_with_actionable_error() {
    let tmp = tempfile::TempDir::new().unwrap();
    let fixture_dir = fixture("agent-plugins-example");
    let installer = FileSystemPluginInstaller::with_base_dir(tmp.path().to_path_buf());
    let installed = installer
        .install(fixture_dir.to_str().unwrap(), Some("dup-plugin".into()))
        .unwrap();
    assert!(installed.path.join("plugin.json").is_file());

    let err = installer
        .install(fixture_dir.to_str().unwrap(), Some("dup-plugin".into()))
        .err()
        .expect("duplicate install must fail");
    match err {
        PluginError::AlreadyInstalled(message) => {
            assert!(message.contains("dup-plugin"), "message should name the plugin: {message}");
            assert!(message.contains("plugins remove"), "message should suggest removal: {message}");
            assert!(message.contains("--name"), "message should suggest --name: {message}");
        }
        other => panic!("expected AlreadyInstalled, got: {other}"),
    }

    installer.remove("dup-plugin").unwrap();
}

#[test]
fn remove_reports_unknown_plugin_with_actionable_error() {
    let tmp = tempfile::TempDir::new().unwrap();
    let installer = FileSystemPluginInstaller::with_base_dir(tmp.path().to_path_buf());
    let err = installer.remove("never-installed-plugin").expect_err("remove must fail");
    match err {
        PluginError::NotInstalled(message) => {
            assert!(message.contains("never-installed-plugin"), "message should name the plugin: {message}");
            assert!(message.contains("plugins add"), "message should suggest install: {message}");
        }
        other => panic!("expected NotInstalled, got: {other}"),
    }
}

#[test]
fn install_refuses_path_traversal_in_name() {
    let tmp = tempfile::TempDir::new().unwrap();
    let fixture_dir = fixture("agent-plugins-example");
    let installer = FileSystemPluginInstaller::with_base_dir(tmp.path().to_path_buf());

    for evil in ["../evil", "../../etc", "..", "foo/../../bar", "/etc/evil"] {
        match installer.install(fixture_dir.to_str().unwrap(), Some(evil.into())) {
            Err(PluginError::InvalidName(_)) => {}
            Ok(_) => panic!("name {evil:?} unexpectedly installed"),
            Err(e) => panic!("name {evil:?} expected InvalidName, got: {e}"),
        }
    }

    // Nothing may have been written outside the plugins root.
    let plugins_root = tmp.path().join(".agents/plugins");
    assert!(!plugins_root.exists() || std::fs::read_dir(&plugins_root).unwrap().next().is_none());
}

#[test]
fn remove_refuses_path_traversal_in_name() {
    let tmp = tempfile::TempDir::new().unwrap();
    let installer = FileSystemPluginInstaller::with_base_dir(tmp.path().to_path_buf());

    // A sentinel file that a traversal remove() must not be able to delete.
    let sentinel = tmp.path().join("sentinel.txt");
    std::fs::write(&sentinel, "keep me").unwrap();

    for evil in ["../sentinel.txt", "../../sentinel.txt", "..", ".", ""] {
        match installer.remove(evil) {
            Err(PluginError::InvalidName(_)) => {}
            Ok(_) => panic!("name {evil:?} unexpectedly removed"),
            Err(e) => panic!("name {evil:?} expected InvalidName, got: {e}"),
        }
    }

    assert!(sentinel.is_file(), "traversal remove() deleted a file outside the plugins root");
}
