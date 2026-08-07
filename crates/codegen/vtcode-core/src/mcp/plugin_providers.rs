use dirs;
use hashbrown::HashMap;
use std::path::Path;
use tracing::warn;

use vtcode_agent_plugins::LoadedPlugin;
use vtcode_config::mcp::McpProviderConfig;

pub fn discover_plugin_mcp_providers(workspace_root: &Path) -> Vec<McpProviderConfig> {
    let mut providers = Vec::new();

    let roots = vec![
        workspace_root.join(".agents/plugins"),
        dirs::home_dir().map(|h| h.join(".agents/plugins")).unwrap_or_default(),
    ];

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let entries = match std::fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::debug!(root = %root.display(), error = %e, "failed to read plugin root directory");
                continue;
            }
        };
        for entry in entries {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            match LoadedPlugin::load_from_dir(&path) {
                Ok(loaded) => {
                    if let Some(mcp_config) = loaded.mcp {
                        for (server_name, server) in mcp_config.servers {
                            match map_server_to_provider(&loaded.manifest.name, &server_name, server, &loaded.root) {
                                Ok(provider) => providers.push(provider),
                                Err(e) => {
                                    warn!(plugin = %loaded.manifest.name, server = %server_name, error = %e, "skipping plugin MCP server")
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(root = %path.display(), error = %e, "failed to load agent plugin for MCP discovery");
                }
            }
        }
    }

    providers
}

fn map_server_to_provider(
    plugin_name: &str,
    server_name: &str,
    server: vtcode_agent_plugins::ServerConfig,
    plugin_root: &Path,
) -> Result<McpProviderConfig, vtcode_agent_plugins::PluginError> {
    let provider_name = format!("{}.{}", plugin_name, server_name);

    match server {
        vtcode_agent_plugins::ServerConfig::Stdio(stdio) => {
            let plugin_data = plugin_root.join("data");
            if let Err(e) = std::fs::create_dir_all(&plugin_data) {
                tracing::warn!(root = %plugin_root.display(), error = %e, "failed to create plugin data directory");
            }

            if stdio.command.starts_with("./") {
                vtcode_agent_plugins::validate_plugin_relative(&stdio.command, plugin_root)
                    .map_err(|e| vtcode_agent_plugins::PluginError::PathEscape(e.to_string()))?;
            }

            let mut env = stdio
                .env
                .into_iter()
                .map(|(k, v)| (k, vtcode_agent_plugins::expand_placeholders(&v, plugin_root, &plugin_data)))
                .collect::<HashMap<String, String>>();
            env.insert("PLUGIN_ROOT".into(), plugin_root.to_string_lossy().to_string());
            env.insert("PLUGIN_DATA".into(), plugin_data.to_string_lossy().to_string());

            let cwd = stdio
                .cwd
                .as_deref()
                .map(|c| vtcode_agent_plugins::expand_placeholders(c, plugin_root, &plugin_data));
            let args = stdio
                .args
                .iter()
                .map(|a| vtcode_agent_plugins::expand_placeholders(a, plugin_root, &plugin_data))
                .collect();

            let cwd = cwd.unwrap_or_else(|| plugin_root.to_string_lossy().to_string());

            Ok(McpProviderConfig {
                name: provider_name,
                transport: vtcode_config::mcp::McpTransportConfig::Stdio(vtcode_config::mcp::McpStdioServerConfig {
                    command: stdio.command,
                    args,
                    working_directory: Some(cwd),
                }),
                env,
                ..McpProviderConfig::default()
            })
        }
        vtcode_agent_plugins::ServerConfig::StreamableHttp(http) => Ok(McpProviderConfig {
            name: provider_name,
            transport: vtcode_config::mcp::McpTransportConfig::Http(vtcode_config::mcp::McpHttpServerConfig {
                endpoint: http.url,
                api_key_env: None,
                oauth: None,
                protocol_version: "2024-11-05".into(),
                http_headers: http.headers.into_iter().collect(),
                env_http_headers: HashMap::new(),
            }),
            ..McpProviderConfig::default()
        }),
        vtcode_agent_plugins::ServerConfig::Sse(_) => {
            Err(vtcode_agent_plugins::PluginError::InvalidMcp("SSE transport is not supported by VT Code".into()))
        }
    }
}
