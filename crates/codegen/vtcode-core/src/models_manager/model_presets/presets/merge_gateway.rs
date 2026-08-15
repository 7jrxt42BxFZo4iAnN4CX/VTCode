use super::super::ModelPreset;
use crate::config::constants::models;
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;

pub(crate) fn merge_gateway_presets() -> Vec<ModelPreset> {
    [
        (
            models::merge_gateway::DEFAULT_ROUTING,
            "Default Routing (Merge Gateway)",
            "Merge Gateway automatically selects a configured route for the request",
            128_000,
            true,
        ),
        (
            models::merge_gateway::OPENAI_GPT_5_5,
            "GPT-5.5 (Merge Gateway)",
            "OpenAI GPT-5.5 through Merge Gateway",
            1_100_000,
            false,
        ),
        (
            models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5,
            "Claude Opus 5 (Merge Gateway)",
            "Anthropic Claude Opus 5 through Merge Gateway",
            1_000_000,
            false,
        ),
        (
            models::merge_gateway::GOOGLE_GEMINI_3_6_FLASH,
            "Gemini 3.6 Flash (Merge Gateway)",
            "Google Gemini 3.6 Flash through Merge Gateway",
            1_000_000,
            false,
        ),
    ]
    .into_iter()
    .map(|(model, display_name, description, context_window, is_default)| ModelPreset {
        id: model.to_string(),
        model: model.to_string(),
        display_name: display_name.to_string(),
        description: description.to_string(),
        provider: Provider::MergeGateway,
        default_reasoning_effort: ReasoningEffortLevel::None,
        supported_reasoning_efforts: Vec::new(),
        is_default,
        upgrade: None,
        show_in_picker: true,
        supported_in_api: true,
        context_window: Some(context_window),
    })
    .collect()
}
