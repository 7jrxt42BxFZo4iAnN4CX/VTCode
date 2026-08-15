//! Merge Gateway OpenAI-compatible provider.

use serde_json::Value;
use vtcode_config::constants::{env_vars, models, urls};

use super::openai_compat::{OpenAiCompatCore, OpenAiCompatSpec, impl_openai_compat_provider};

/// Static configuration for Merge Gateway's OpenAI-compatible Chat
/// Completions endpoint.
pub struct MergeGatewaySpec;

fn no_reasoning(_message: &Value, _choice: &Value) -> Option<String> {
    None
}

impl OpenAiCompatSpec for MergeGatewaySpec {
    const NAME: &'static str = "Merge Gateway";
    const KEY: &'static str = "merge-gateway";
    const API_KEY_ENV: &'static str = env_vars::MERGE_GATEWAY_API_KEY;
    const DEFAULT_MODEL: &'static str = models::merge_gateway::DEFAULT_MODEL;
    const DEFAULT_BASE_URL: &'static str = urls::MERGE_GATEWAY_API_BASE;
    const BASE_URL_ENV: Option<&'static str> = Some(env_vars::MERGE_GATEWAY_BASE_URL);
    const LISTED_MODELS: &'static [&'static str] = models::merge_gateway::SUPPORTED_MODELS;
    // Merge's model catalog is larger and changes independently of VT Code's
    // curated picker entries. Explicit provider/model IDs must pass through.
    const VALIDATION_ALLOWLIST: Option<&'static [&'static str]> = None;
    const STREAM_OPTIONS_INCLUDE_USAGE: bool = true;
    const SUPPRESS_SAMPLING_WHEN_REASONING: bool = false;
    const STREAM_REASONING_FIELDS: &'static [&'static str] = &[];
    const DELTA_ORDER: super::shared::OpenAiDeltaOrder = super::shared::OpenAiDeltaOrder::ContentFirst;
    // Route-specific reasoning is intentionally not inferred from the
    // compatibility response shape.
    const RESPONSE_REASONING_EXTRACTOR: Option<super::openai_compat::ReasoningExtractor> = Some(no_reasoning);

    fn resolve_api_key(api_key: Option<String>) -> String {
        api_key
            .or_else(|| std::env::var(Self::API_KEY_ENV).ok().filter(|key| !key.trim().is_empty()))
            .unwrap_or_default()
    }
}

impl_openai_compat_provider!(MergeGatewayProvider, MergeGatewaySpec, {
    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_structured_output(&self, _model: &str) -> bool {
        false
    }

    fn supports_reasoning(&self, _model: &str) -> bool {
        false
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        false
    }

    fn supports_vision(&self, model: &str) -> bool {
        matches!(
            model,
            models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5 | models::merge_gateway::GOOGLE_GEMINI_3_6_FLASH
        )
    }

    fn effective_context_size(&self, model: &str) -> usize {
        match model {
            models::merge_gateway::OPENAI_GPT_5_5 => 1_100_000,
            models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5 | models::merge_gateway::GOOGLE_GEMINI_3_6_FLASH => {
                1_000_000
            }
            _ => 128_000,
        }
    }
});

#[cfg(test)]
mod tests {
    use super::{MergeGatewayProvider, MergeGatewaySpec};
    use crate::BackendKind;
    use crate::provider::{LLMProvider, LLMRequest, Message};
    use crate::providers::openai_compat::OpenAiCompatSpec;
    use serde_json::json;
    use std::sync::Arc;
    use vtcode_config::constants::{models, urls};

    #[test]
    fn merge_gateway_uses_default_endpoint_and_route_model() {
        let provider =
            MergeGatewayProvider::from_config(Some("test-key".to_string()), None, None, None, None, None, None);

        assert_eq!(provider.name(), "merge-gateway");
        assert_eq!(provider.backend_kind(), BackendKind::MergeGateway);
        assert_eq!(provider.core.base_url, urls::MERGE_GATEWAY_API_BASE);
        assert_eq!(provider.core.model, models::merge_gateway::DEFAULT_ROUTING);
        assert_eq!(MergeGatewaySpec::API_KEY_ENV, "MERGE_GATEWAY_API_KEY");

        let overridden = MergeGatewayProvider::from_config(
            Some("test-key".to_string()),
            None,
            Some("https://merge-proxy.example/v1/openai".to_string()),
            None,
            None,
            None,
            None,
        );
        assert_eq!(overridden.core.base_url, "https://merge-proxy.example/v1/openai");
    }

    #[test]
    #[serial_test::serial]
    #[allow(
        unsafe_code,
        reason = "Rust 2024 requires unsafe process-environment mutation in this test"
    )]
    fn merge_gateway_falls_back_to_api_key_environment() {
        let previous = std::env::var(MergeGatewaySpec::API_KEY_ENV).ok();
        // Environment mutation is isolated to this serial test because the
        // provider intentionally follows the process-level API convention.
        // SAFETY: This test is serialized and restores the previous value before returning.
        unsafe { std::env::set_var(MergeGatewaySpec::API_KEY_ENV, "env-key") };

        let provider = MergeGatewayProvider::from_config(None, None, None, None, None, None, None);
        assert_eq!(provider.core.api_key, "env-key");

        match previous {
            Some(value) => {
                // SAFETY: This test is serialized and restores the value captured above.
                unsafe { std::env::set_var(MergeGatewaySpec::API_KEY_ENV, value) }
            }
            None => {
                // SAFETY: This test is serialized and removes only the variable it set.
                unsafe { std::env::remove_var(MergeGatewaySpec::API_KEY_ENV) }
            }
        }
    }

    #[test]
    fn merge_gateway_emits_chat_payload_for_default_route() {
        let provider = MergeGatewayProvider::from_config(
            Some("test-key".to_string()),
            Some(models::merge_gateway::DEFAULT_ROUTING.to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        let request = LLMRequest {
            messages: vec![Message::user("hello".to_string())].into(),
            model: models::merge_gateway::DEFAULT_ROUTING.to_string(),
            stream: true,
            ..Default::default()
        };

        let payload = provider.core.convert_request(&request).expect("payload should be valid");
        assert_eq!(payload["model"], models::merge_gateway::DEFAULT_ROUTING);
        assert_eq!(payload["stream"], true);
        assert_eq!(payload["stream_options"]["include_usage"], true);
        assert_eq!(payload.get("reasoning_effort"), None);
        assert_eq!(payload.get("reasoning_content"), None);
        assert_eq!(json!(provider.supported_models()), json!(models::merge_gateway::SUPPORTED_MODELS));
    }

    #[test]
    fn merge_gateway_accepts_arbitrary_explicit_model_ids() {
        let provider = MergeGatewayProvider::from_config(
            Some("test-key".to_string()),
            Some(models::merge_gateway::DEFAULT_ROUTING.to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        let request = LLMRequest {
            messages: vec![Message::user("hello".to_string())].into(),
            model: "deepseek/deepseek-v4-pro".to_string(),
            ..Default::default()
        };

        provider
            .validate_request(&request)
            .expect("valid Merge routes should pass through local validation");
    }

    #[test]
    fn merge_gateway_serializes_tools_without_reasoning_fields() {
        let provider = MergeGatewayProvider::from_config(
            Some("test-key".to_string()),
            Some(models::merge_gateway::DEFAULT_ROUTING.to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        let request = LLMRequest {
            messages: vec![Message::user("hello".to_string())].into(),
            model: models::merge_gateway::DEFAULT_ROUTING.to_string(),
            tools: Some(Arc::new(vec![crate::provider::ToolDefinition::function(
                "get_weather".to_string(),
                "Get the weather".to_string(),
                json!({"type": "object", "properties": {"city": {"type": "string"}}}),
            )])),
            ..Default::default()
        };

        let payload = provider.core.convert_request(&request).expect("payload should be valid");
        assert_eq!(payload["tools"][0]["type"], "function");
        assert!(payload.get("reasoning_effort").is_none());
    }

    #[test]
    fn merge_gateway_capabilities_are_conservative() {
        let provider = MergeGatewayProvider::from_config(
            Some("test-key".to_string()),
            Some(models::merge_gateway::DEFAULT_ROUTING.to_string()),
            None,
            None,
            None,
            None,
            None,
        );

        assert!(provider.supports_streaming());
        assert!(provider.supports_tools(models::merge_gateway::DEFAULT_ROUTING));
        assert!(!provider.supports_reasoning(models::merge_gateway::DEFAULT_ROUTING));
        assert!(!provider.supports_reasoning_effort(models::merge_gateway::DEFAULT_ROUTING));
        assert!(provider.supports_vision(models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5));
        assert!(!provider.supports_vision(models::merge_gateway::DEFAULT_ROUTING));
        assert_eq!(provider.effective_context_size("merge/custom-route"), 128_000);
    }
}
