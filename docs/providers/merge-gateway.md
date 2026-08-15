# Merge Gateway Integration

Merge Gateway gives VT Code one OpenAI-compatible endpoint for routing requests
to Merge's supported model vendors. VT Code integrates the compatibility surface
at `/v1/openai` and keeps Merge's route-specific controls out of the generic
provider contract.

## Setup

1. Create a Merge API key in the [Merge dashboard](https://dashboard.merge.dev/).
2. Export it before starting VT Code:

   ```bash
   export MERGE_GATEWAY_API_KEY="your-merge-api-key"
   ```

3. Select the provider in `vtcode.toml`:

   ```toml
   [agent]
   provider = "merge-gateway"
   default_model = "default_routing"
   api_key_env = "MERGE_GATEWAY_API_KEY"
   ```

The default endpoint is:

```text
https://api-gateway.merge.dev/v1/openai
```

Set `MERGE_GATEWAY_BASE_URL` when a proxy or compatible gateway endpoint is
required. The value should remain the provider base URL; VT Code appends
`/chat/completions` for requests.

## Quick start

```bash
export MERGE_GATEWAY_API_KEY="your-merge-api-key"
vtcode --provider merge-gateway --model default_routing
```

Use an explicit route when you want to select a vendor model:

```bash
vtcode --provider merge-gateway --model anthropic/claude-opus-5
```

## Curated models

| Model ID | Context | Vision metadata | Notes |
| --- | ---: | :---: | --- |
| `default_routing` | 128k baseline | No | Merge chooses the route |
| `openai/gpt-5.5` | 1.1M | No | OpenAI route |
| `anthropic/claude-opus-5` | 1M | Yes | Anthropic route |
| `google/gemini-3.6-flash` | 1M | Yes | Google route |

These are the models shown in VT Code's picker. Merge model IDs are not a
closed local allowlist: any valid explicit `provider/model` route can be used
with `provider = "merge-gateway"`, even when it is not in `docs/models.json`.

## Configuration examples

Provider settings can make the endpoint and credential identity explicit:

```toml
[agent.provider_settings.merge-gateway]
name = "Merge Gateway"
base_url = "https://api-gateway.merge.dev/v1/openai"
env_key = "MERGE_GATEWAY_API_KEY"
```

The environment variables are:

| Variable | Purpose |
| --- | --- |
| `MERGE_GATEWAY_API_KEY` | Bearer token used for Merge Gateway requests |
| `MERGE_GATEWAY_BASE_URL` | Optional override for the OpenAI-compatible base URL |

## Compatibility behavior

VT Code uses the shared OpenAI-compatible Chat Completions transport. The
integration supports bearer authentication, streaming, usage reporting in
streaming requests, standard tool serialization, and the normal message
format. It does not add a native Merge Responses implementation.

Merge's reasoning controls vary by route and vendor. For that reason VT Code
does not send a generic `reasoning_effort` field and does not infer
`reasoning_content` from compatibility responses. Configure reasoning through
the selected Merge route's documented behavior.

The local catalog supplies conservative capability metadata for the curated
routes. It does not promise that every route supports vision, structured
output, or the same context window. Prompt-cache session headers, routing
metadata, service tiers, Gateway-controlled thinking, and per-call cost
extraction remain outside this compatibility-only integration.

## Troubleshooting

- `401` or authentication errors: confirm `MERGE_GATEWAY_API_KEY` is set and
  points to a key created in Merge.
- `404` errors: verify the base URL ends at `/v1/openai`, not `/v1` or
  `/chat/completions`; VT Code appends the chat-completions path.
- A model is rejected by Merge: confirm the exact vendor-prefixed route ID in
  Merge's catalog. VT Code deliberately does not reject unknown Merge IDs
  locally.
- Reasoning output is absent: this is expected for the compatibility adapter;
  Merge reasoning controls are route-specific and are not projected into the
  generic VT Code reasoning fields.

See the [Merge Gateway quick reference](./merge-gateway-quick-reference.md) for
the shortest setup checklist.
