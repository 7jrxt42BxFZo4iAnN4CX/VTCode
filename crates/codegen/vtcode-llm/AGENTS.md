# vtcode-llm

[Root AGENTS.md](../AGENTS.md) | **Canonical** LLM provider trait, types, and implementations.

## Key Modules

`provider/` trait + shared types | `providers/` per-provider impls | `provider.rs` re-exports | `client.rs` + `optimized_client.rs` | `copilot/` (feature-gated) | `open_responses/` | `factory_types.rs` + `provider_config_types.rs` config | `system_prompt.rs` injection | `http_client.rs` | `types.rs` shared types | `utils.rs` + `single_response.rs` + `tool_bridge.rs` + `config_adapter.rs` + `rig_adapter.rs` + `provider_base.rs` + `error_display.rs` + `model_resolver.rs` infra (merged from core)

## Architecture Notes

- **Canonical home** for all provider code. Core's `llm/` is a thin re-export layer + factory/CGP.
- `ModelResolver::resolve_with_mode` and `availability_with_mode` must receive the loaded config's credential storage mode; compatibility wrappers are only for callers without workspace config.
- `ResolvedModel.api_key_env` carries provider-override credential identity through availability and picker selection; do not infer availability from provider alone.
- `system_prompt.rs` provides stub getters with `OnceLock` setters; vtcode-core overrides at init.
- Uses `compact_str::CompactString` (aliased `CompactStr` from `vtcode_core::types`) for small string fields.

## Dependencies

`vtcode-commons` (HTTP, CGP, types) | `vtcode-config` (provider config, timeouts) | `vtcode-utility-tool-specs` (schemas) | `vtcode-exec-events` | `vtcode-macros`

## Coding Conventions

Providers in `providers/<name>/mod.rs`. Use `anyhow::Result`, `tracing`, not `println!`. Provider-specific types stay local; shared go in `types.rs` or `provider/`.

## OpenAI-Compatible Providers

- `providers/openai_compat.rs` owns the shared shell: `OpenAiCompatSpec` (per-provider consts/overrides) + `OpenAiCompatCore<S>` + `impl_openai_compat_provider!`. New compat providers implement a Spec (~50-200 lines), not a full `LLMProvider`.
- Model normalization happens in `core.prepare()`, not `convert_request()` — payload tests must call `prepare` first. `stream: true` is only inserted when `request.stream` is set.
- Providers with extra protocols (evolink Anthropic path, opencode) hand-write the provider over `OpenAiCompatCore` instead of using the macro.
- Registration contract: keep the type name and 7-arg `from_config` consumed by `impl_standard_provider_constructor!` in vtcode-core. The Open Responses bridge maps authoritative plan-approval `ThreadEvent` variants to `vtcode.*` custom events for client parity.
