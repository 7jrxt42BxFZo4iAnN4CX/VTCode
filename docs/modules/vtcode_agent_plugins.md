# vtcode-agent-plugins

Agent Plugins 1.0.0 manifest parsing, validation, and discovery for VT Code.

## Overview

Implements a conformant [Agent Plugins 1.0.0](https://agent-plugins.org/specification) client. A plugin is a directory containing a root `plugin.json` manifest, optional `skills/*/SKILL.md` Agent Skills, and optional `mcp.json` MCP server configuration. The crate parses and validates manifests, discovers bundled skills and MCP servers, expands plugin environment placeholders, and enforces path containment.

## Module Groups

| Area | Modules | Description |
|------|---------|-------------|
| Manifest | `manifest.rs` | `PluginManifest` parsing and validation |
| Discovery | `discovery.rs` | `LoadedPlugin` load, skill/MCP discovery |
| MCP | `mcp.rs` | `mcp.json` parsing into `ServerConfig` |
| Loading | `loader.rs` | `PluginLoader` / `PluginInstaller` traits + filesystem impls |
| Expansion | `expansion.rs` | `PLUGIN_ROOT` / `PLUGIN_DATA` placeholder expansion |
| Errors | `errors.rs` | `PluginError` diagnostics |

## Key Components

### Manifest

`PluginManifest::parse` requires `$schema` and `name`; all other fields are optional. Unknown top-level fields are non-fatal (reported via `unknown_fields`). Names must be 1-64 chars of `a-z`, `0-9`, `-`, `.`, start and end alphanumeric, and contain no `--` or `..`.

### LoadedPlugin

`LoadedPlugin::load_from_dir` reads `plugin.json`, discovers `skills/*/SKILL.md` (immediate children only), and parses `mcp.json`. Each skill must pass the strict Agent Skills validation, and its `name` must match its parent directory. An `mcp.json` schema version that differs from `plugin.json` disables MCP for that plugin.

### Loader and Installer

- `PluginLoader` / `FileSystemPluginLoader` — load a plugin from a directory.
- `PluginInstaller` / `FileSystemPluginInstaller` — `install` clones a git URL (`--depth=1`) or copies a local directory into `~/.agents/plugins/<name>`, then loads and validates the result; `remove` deletes an installed plugin.

### Expansion

For stdio MCP servers VT Code injects `PLUGIN_ROOT` and `PLUGIN_DATA` environment variables and expands `${PLUGIN_ROOT}` / `${PLUGIN_DATA}` placeholders in `args`, `env`, and `cwd`. `validate_plugin_relative` rejects `./`-prefixed paths that escape the plugin root.

## Runtime Integration

- Skills: `vtcode-core::skills::loader` discovers plugin skills from `<workspace>/.agents/plugins`.
- MCP: `vtcode-core::mcp::plugin_providers::discover_plugin_mcp_providers` surfaces plugin MCP servers as `<plugin>.<server>` providers at session startup, from both the workspace and user plugin roots.

## See Also

- [Agent Plugins Guide](../guides/agent-plugins.md) — setup and usage
- [Agent Plugins User Guide](../user-guide/agent-plugins.md) — task-oriented quick start
- [MCP Integration Guide](../guides/mcp-integration.md) — MCP client and server modes
- [Agent Skills Guide](../skills/SKILLS_GUIDE.md) — creating and loading skills
