# Merge Gateway Quick Reference

| Setting | Value |
| --- | --- |
| Provider key | `merge-gateway` |
| API key | `MERGE_GATEWAY_API_KEY` |
| Default endpoint | `https://api-gateway.merge.dev/v1/openai` |
| Endpoint override | `MERGE_GATEWAY_BASE_URL` |
| Default model | `default_routing` |
| Curated routes | `openai/gpt-5.5`, `anthropic/claude-opus-5`, `google/gemini-3.6-flash` |
| Transport | OpenAI-compatible Chat Completions |
| Authentication | Bearer token |
| Tool calls | Supported |
| Streaming usage | Supported via `stream_options.include_usage` |
| Reasoning effort | Not forwarded; controls are route-specific |

## Minimal setup

```bash
export MERGE_GATEWAY_API_KEY="your-merge-api-key"
vtcode --provider merge-gateway --model default_routing
```

For arbitrary valid Merge routes:

```bash
vtcode --provider merge-gateway --model deepseek/deepseek-v4-pro
```

Create keys in the [Merge dashboard](https://dashboard.merge.dev/). See the
[full Merge Gateway guide](./merge-gateway.md) for configuration, limitations,
and troubleshooting.
