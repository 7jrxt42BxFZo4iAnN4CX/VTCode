# vtcode-config

[Root AGENTS.md](../AGENTS.md) | Config loading, schema, constants. `vtcode.toml` is the source of truth.

## Modules

`loader/` ConfigManager + ConfigBuilder + layers | `constants/` models, env vars, URLs, tools, and shared tool limits | `core/` AgentConfig + all nested config structs | `models/` ModelId + Provider enums | `types/` ReasoningEffortLevel and related enums | `schema/` JSON Schema export (feature-gated) | `defaults/` ConfigDefaultsProvider | `auth/` auth config re-exports | `mcp/` MCP config | `acp/` ACP config | `hooks/` lifecycle hooks | `subagents/` subagent discovery | `core/network_allowlist.rs` | `core/provider_override.rs`

## Rules

- `ModelId` enum is the canonical model identifier — all model matching must go through it.
- `constants/` is organized by domain: `models/`, `urls.rs`, `env_vars.rs`, `tools.rs`.
- `ConfigLayerStack` handles layered config (defaults → file → env → CLI) — do not bypass.
- `bootstrap` feature (default) scaffolds config dirs. Disable for parse-only consumers.
- `schema` feature gates `vtcode_config_schema_json()` — used by `build.rs`.

## Adding a Model

Two pathways: **OpenRouter** (code-generated) — edit `ModelId`, `Provider::OpenRouter` match, `docs/models.json`. Build script handles the rest. **Non-OpenRouter** (manual) — add constant, `ModelId` variant + all match arms, defaults if needed, preset, optional resolver update, `docs/models.json`. See `adding-llm-providers` skill for checklist.

## Gotchas

- `VTCodeConfig::load()` resolves layers — do not use `ConfigManager::load_from_workspace()` directly in production code.
- `models/model_id/table.rs` (`model_id_table!`) is the single source for as_str/parse/display/description/provider per variant — add new models as one table row, never a new match arm in the wrapper files.
- `parse.rs` keeps an order-sensitive hand-written preamble (opencode/evolink prefix routing, ZAI shadow guards, dated-haiku remap) before the table lookup — never move prefix rules into the table.
- `core/automation.rs` holds the loop-engineering config surface: `LoopEngineConfig` (gated by `loop_engine_enabled()`, override with `VTCODE_DISABLE_LOOP_ENGINE`), and `verify_mutations` on `FullAutoConfig` — **default off** because the verifier sub-agent doubles mutating-call cost.
- `AgentHarnessConfig` now has `context_reset_mode` (`off`/`on_stall`/`on_compaction`) and `context_reset_stall_threshold` — distinct from compaction config. Default: `off`.
- `constants::tool_limits` is the source of truth for execution-loop defaults, planning/approved-plan floors, and tool-loop extension caps; `agent.max_conversation_turns` remains context retention only.
- Token-efficiency defaults: `tool_result_clearing.enabled` defaults to `true` (old tool results stripped past `trigger_tokens`), `tools.client_tool_search` defaults to `true` (client-local deferral for providers without hosted tool search), and `agent.system_prompt_mode`/`tool_documentation_mode` stay `default`/`progressive`. When changing these, update `docs/config/CONFIG_FIELD_REFERENCE.md` and the guard-rail tests. `agent.ui_surface` defaults to `inline` so interactive plan/interview HITL overlays work without configuration; `auto` and `alternate` remain explicit overrides.
- Use `api_keys::get_api_key_with_mode` and `provider_credential_detail_with_mode` when a loaded config is available; OpenAI runtime auth also uses `resolve_openai_api_key_for_auth`. Compatibility wrappers use the platform default storage mode.
