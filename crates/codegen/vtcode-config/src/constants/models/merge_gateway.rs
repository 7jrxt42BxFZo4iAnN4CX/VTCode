//! Curated Merge Gateway model routes exposed by VT Code.

pub const DEFAULT_ROUTING: &str = "default_routing";
pub const OPENAI_GPT_5_5: &str = "openai/gpt-5.5";
pub const ANTHROPIC_CLAUDE_OPUS_5: &str = "anthropic/claude-opus-5";
pub const GOOGLE_GEMINI_3_6_FLASH: &str = "google/gemini-3.6-flash";

pub const DEFAULT_MODEL: &str = DEFAULT_ROUTING;

/// Curated routes shown in VT Code's model picker. Merge Gateway also accepts
/// other valid `provider/model` identifiers through explicit configuration.
pub const SUPPORTED_MODELS: &[&str] = &[
    DEFAULT_ROUTING,
    OPENAI_GPT_5_5,
    ANTHROPIC_CLAUDE_OPUS_5,
    GOOGLE_GEMINI_3_6_FLASH,
];

/// Merge controls reasoning per route, so VT Code does not forward a
/// provider-wide reasoning effort parameter for these models.
pub const REASONING_MODELS: &[&str] = &[];
