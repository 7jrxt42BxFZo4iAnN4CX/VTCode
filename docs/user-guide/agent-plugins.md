# Agent Plugins

Agent Plugins are portable packages that bundle Agent Skills and MCP servers under a root `plugin.json` manifest, following the [Agent Plugins 1.0.0](https://agent-plugins.org/specification) spec. Install one and its skills and MCP servers become available to VT Code immediately.

For the full reference — plugin layout, manifest fields, MCP transports, and security behavior — see the [Agent Plugins Guide](../guides/agent-plugins.md).

## What a plugin gives you

A plugin directory looks like:

```text
my-plugin/
├── plugin.json              # required: $schema + name
├── skills/
│   └── my-skill/
│       └── SKILL.md         # Agent Skill (name must match its folder)
└── mcp.json                 # optional MCP servers
```

- **Skills** placed under `skills/*/SKILL.md` are discovered from `<workspace>/.agents/plugins` and appear alongside your regular skills.
- **MCP servers** declared in `mcp.json` are started at session startup and exposed as `<plugin>.<server>` providers.

## Install a plugin

```bash
vtcode plugins add https://github.com/example/my-plugin
```

`add` accepts a git URL or a local directory, clones/copies it into `~/.agents/plugins/<name>`, and validates it. Pass `--name` to override the directory name:

```bash
vtcode plugins add https://github.com/example/my-plugin --name my-tools
```

`add` installs to your **user** plugin root (`~/.agents/plugins`). To keep a plugin project-local instead, place it (or clone it) under `<workspace>/.agents/plugins/<name>` yourself.

## Manage plugins

```bash
vtcode plugins list                   # installed plugins (skills + MCP counts)
vtcode plugins info my-plugin         # details: skills, MCP servers, root
vtcode plugins validate ./my-plugin   # check a plugin directory
vtcode plugins remove my-plugin       # uninstall from ~/.agents/plugins
```

## Use plugin skills and MCP

Once installed, no further setup is needed:

- **Plugin skills** show up in the `/skills` command and are available to the agent like any other skill.
- **Plugin MCP servers** appear in `/mcp` as `<plugin>.<server>` and connect automatically at session start.

Plugin MCP servers are discovered from both `<workspace>/.agents/plugins` and `~/.agents/plugins`. Skills are discovered from the workspace plugin root only.

## Configure plugin behavior

Control plugins through the `tools.plugins` section of `vtcode.toml`:

```toml
[tools.plugins]
enabled = true                # toggle the plugin runtime
default_trust = "sandbox"     # sandbox | trusted | untrusted
allow = ["my-plugin"]         # optional allow-list
deny = []                     # optional block-list
auto_reload = true            # hot-reload manifest polling
```

See the [Configuration Field Reference](../config/CONFIG_FIELD_REFERENCE.md) for every `tools.plugins.*` option.

## Further reading

- [Agent Plugins Guide](../guides/agent-plugins.md) — reference: layout, manifest validation, MCP transports, environment expansion, path containment
- [MCP Integration Guide](../guides/mcp-integration.md) — using MCP servers in general
- [Agent Skills Guide](../skills/SKILLS_GUIDE.md) — creating and loading skills
