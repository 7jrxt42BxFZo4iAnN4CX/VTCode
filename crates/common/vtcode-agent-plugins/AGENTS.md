# vtcode-agent-plugins

Agent Plugins manifest parsing, validation, and discovery for VT Code.

## Conventions

- `PluginManifest` and `McpConfig` are passive data containers; all validation is explicit in `parse()` and returns diagnostics. Wrong-typed fields and unsupported `$schema` URLs are rejected, not silently dropped.
- Unknown top-level `plugin.json` fields are non-fatal (reported and ignored); unknown top-level `mcp.json` fields disable MCP for that plugin only.
- Skill discovery from a plugin is bounded to `skills/*/SKILL.md` (immediate children only), per the Agent Plugins spec. A broken skill is skipped with a warning, never fatal to the whole plugin.
- Any user-controlled name or path that is joined onto the plugins root must go through `validate_name` / `validate_plugin_relative` (canonicalize-based) first — `install`, `remove`, and MCP `command`/`cwd` are the security boundaries. Directory copies re-create symlinks, skip dotfiles, and abort on cycles.

## Dependencies

- `vtcode-skills` (SKILL.md frontmatter parsing and `SkillManifest::validate`)
- `vtcode-commons` (filesystem helpers)
