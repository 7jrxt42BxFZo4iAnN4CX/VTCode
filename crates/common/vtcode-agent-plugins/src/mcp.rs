use std::collections::HashMap;

use crate::errors::PluginError;

const SUPPORTED_SCHEMAS: &[&str] = &["https://agent-plugins.org/schemas/1.0.0/mcp.schema.json"];

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
        if !SUPPORTED_SCHEMAS.contains(&config.schema.as_str()) {
            return Err(PluginError::InvalidMcp(format!(
                "unsupported $schema '{}' (expected {})",
                config.schema,
                SUPPORTED_SCHEMAS.join(" or ")
            )));
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
                .map(|v| expect_string_array(v, &format!("server '{name}' args")))
                .transpose()?
                .unwrap_or_default();

            let env = obj
                .get("env")
                .map(|v| expect_string_map(v, &format!("server '{name}' env")))
                .transpose()?
                .unwrap_or_default();

            let cwd = obj
                .get("cwd")
                .map(|v| {
                    v.as_str()
                        .map(String::from)
                        .ok_or_else(|| PluginError::InvalidMcp(format!("server '{name}' cwd must be a string")))
                })
                .transpose()?;

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
                .map(|v| expect_string_map(v, &format!("server '{name}' headers")))
                .transpose()?
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
                .map(|v| expect_string_map(v, &format!("server '{name}' headers")))
                .transpose()?
                .unwrap_or_default();

            Ok(ServerConfig::Sse(SseServerConfig { url, headers }))
        }
        _ => Err(PluginError::InvalidMcp(format!("server '{}' has unsupported type: {}", name, type_val))),
    }
}

fn expect_string_array(value: &serde_json::Value, context: &str) -> Result<Vec<String>, PluginError> {
    value
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|v| {
                    v.as_str()
                        .map(String::from)
                        .ok_or_else(|| PluginError::InvalidMcp(format!("{context} must contain only strings")))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap_or_else(|| Err(PluginError::InvalidMcp(format!("{context} must be an array"))))
}

fn expect_string_map(value: &serde_json::Value, context: &str) -> Result<HashMap<String, String>, PluginError> {
    value
        .as_object()
        .map(|obj| {
            obj.iter()
                .map(|(k, v)| {
                    v.as_str()
                        .map(|s| (k.clone(), s.to_string()))
                        .ok_or_else(|| PluginError::InvalidMcp(format!("{context}.{k} must be a string")))
                })
                .collect::<Result<HashMap<_, _>, _>>()
        })
        .unwrap_or_else(|| Err(PluginError::InvalidMcp(format!("{context} must be an object"))))
}
