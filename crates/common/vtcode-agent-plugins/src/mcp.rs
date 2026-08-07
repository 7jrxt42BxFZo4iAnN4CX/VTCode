use std::collections::HashMap;

use crate::errors::PluginError;

#[derive(Debug, Clone)]
pub struct McpConfig {
    pub schema: String,
    pub servers: HashMap<String, ServerConfig>,
    pub unknown_fields: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum ServerConfig {
    Stdio(StdioServerConfig),
    StreamableHttp(HttpServerConfig),
    Sse(SseServerConfig),
}

#[derive(Debug, Clone)]
pub struct StdioServerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HttpServerConfig {
    pub url: String,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct SseServerConfig {
    pub url: String,
    pub headers: HashMap<String, String>,
}

impl McpConfig {
    pub fn parse(content: &str) -> Result<Self, PluginError> {
        let value: serde_json::Value = serde_json::from_str(content).map_err(PluginError::Json)?;
        let map = value
            .as_object()
            .ok_or_else(|| PluginError::InvalidMcp("mcp.json must be a JSON object".into()))?;

        let mut unknown = Vec::new();
        let mut config = McpConfig {
            schema: String::new(),
            servers: HashMap::new(),
            unknown_fields: Vec::new(),
        };

        for (key, val) in map {
            match key.as_str() {
                "$schema" => {
                    let s = val
                        .as_str()
                        .ok_or_else(|| PluginError::InvalidMcp("$schema must be a string".into()))?;
                    config.schema = s.to_string();
                }
                "mcpServers" => {
                    let servers_map = val
                        .as_object()
                        .ok_or_else(|| PluginError::InvalidMcp("mcpServers must be an object".into()))?;
                    for (name, server_val) in servers_map {
                        match parse_server_config(name, server_val) {
                            Ok(server) => {
                                config.servers.insert(name.clone(), server);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    server = name,
                                    error = %e,
                                    "skipping invalid MCP server entry in plugin"
                                );
                            }
                        }
                    }
                }
                _ => unknown.push(key.clone()),
            }
        }

        if config.schema.is_empty() {
            return Err(PluginError::InvalidMcp("missing required field: $schema".into()));
        }

        config.unknown_fields = unknown;
        Ok(config)
    }
}

fn parse_server_config(name: &str, value: &serde_json::Value) -> Result<ServerConfig, PluginError> {
    let obj = value
        .as_object()
        .ok_or_else(|| PluginError::InvalidMcp(format!("server '{}' must be an object", name)))?;

    let type_val = obj
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| PluginError::InvalidMcp(format!("server '{}' missing required field: type", name)))?;

    match type_val {
        "stdio" => {
            let command = obj
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or_else(|| PluginError::InvalidMcp(format!("server '{}' missing required field: command", name)))?
                .to_string();

            let args = obj
                .get("args")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();

            let env = obj
                .get("env")
                .and_then(|v| v.as_object())
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();

            let cwd = obj.get("cwd").and_then(|v| v.as_str()).map(String::from);

            Ok(ServerConfig::Stdio(StdioServerConfig { command, args, env, cwd }))
        }
        "streamable-http" => {
            let url = obj
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| PluginError::InvalidMcp(format!("server '{}' missing required field: url", name)))?
                .to_string();

            let headers = obj
                .get("headers")
                .and_then(|v| v.as_object())
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();

            Ok(ServerConfig::StreamableHttp(HttpServerConfig { url, headers }))
        }
        "sse" => {
            let url = obj
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| PluginError::InvalidMcp(format!("server '{}' missing required field: url", name)))?
                .to_string();

            let headers = obj
                .get("headers")
                .and_then(|v| v.as_object())
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();

            Ok(ServerConfig::Sse(SseServerConfig { url, headers }))
        }
        _ => Err(PluginError::InvalidMcp(format!("server '{}' has unsupported type: {}", name, type_val))),
    }
}
