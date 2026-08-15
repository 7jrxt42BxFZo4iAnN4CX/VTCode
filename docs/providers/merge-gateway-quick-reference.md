# Merge Gateway Quick Reference

| Setting | Value |
| --- | --- |
| Provider key | `merge-gateway` |
| API key | `MERGE_GATEWAY_API_KEY` |
| Default endpoint | `https://api-gateway.merge.dev/v1/openai` |
| Endpoint override | `MERGE_GATEWAY_BASE_URL` |
| Default model | `default_routing` |
| Curated routes | See the full list below; all are available in the model picker |
| Transport | OpenAI-compatible Chat Completions |
| Authentication | Bearer token |
| Tool calls | Supported |
| Streaming usage | Supported via `stream_options.include_usage` |
| Reasoning effort | Not forwarded; controls are route-specific |

Curated routes:

```text
openai/gpt-5.5
anthropic/claude-opus-5
google/gemini-3.6-flash
google/gemini-3.7-flash
deepseek/deepseek-v4-pro-0813
deepseek/deepseek-v4-flash-0731
xai/grok-4.6
qwen/qwen3.8-max
minimax/minimax-h3
moonshot/kimi-k3
thinkingmachines/inkling
meta/muse-spark-1.1
openai/gpt-5.6-luna
openai/gpt-5.6-sol
openai/gpt-5.6-terra
```

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
