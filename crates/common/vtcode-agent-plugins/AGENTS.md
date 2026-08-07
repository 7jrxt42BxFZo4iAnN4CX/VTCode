# vtcode-agent-plugins

Agent Plugins 1.0.0 manifest parsing, validation, and discovery for VT Code.

## Conventions

- `PluginManifest` and `McpConfig` are passive data containers; all validation is explicit in `parse()` and returns diagnostics.
- Unknown top-level `plugin.json` fields are non-fatal (reported and ignored); unknown top-level `mcp.json` fields disable MCP for that plugin only.
- Skill discovery from a plugin is bounded to `skills/*/SKILL.md` (immediate children only), per the Agent Plugins spec.

## Dependencies

- `vtcode-skills` (SKILL.md frontmatter parsing and `SkillManifest::validate`)
- `vtcode-commons` (filesystem helpers)
