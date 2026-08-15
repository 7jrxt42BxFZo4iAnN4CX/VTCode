//! Curated Merge Gateway model routes exposed by VT Code.

pub const DEFAULT_ROUTING: &str = "default_routing";
pub const OPENAI_GPT_5_5: &str = "openai/gpt-5.5";
pub const ANTHROPIC_CLAUDE_OPUS_5: &str = "anthropic/claude-opus-5";
pub const GOOGLE_GEMINI_3_6_FLASH: &str = "google/gemini-3.6-flash";
pub const GOOGLE_GEMINI_3_7_FLASH: &str = "google/gemini-3.7-flash";
pub const DEEPSEEK_V4_PRO_0813: &str = "deepseek/deepseek-v4-pro-0813";
pub const DEEPSEEK_V4_FLASH_0731: &str = "deepseek/deepseek-v4-flash-0731";
pub const XAI_GROK_4_6: &str = "xai/grok-4.6";
pub const QWEN_3_8_MAX: &str = "qwen/qwen3.8-max";
pub const MINIMAX_H3: &str = "minimax/minimax-h3";
pub const MOONSHOT_KIMI_K3: &str = "moonshot/kimi-k3";
pub const THINKINGMACHINES_INKLING: &str = "thinkingmachines/inkling";
pub const META_MUSE_SPARK_1_1: &str = "meta/muse-spark-1.1";
pub const OPENAI_GPT_5_6_LUNA: &str = "openai/gpt-5.6-luna";
pub const OPENAI_GPT_5_6_SOL: &str = "openai/gpt-5.6-sol";
pub const OPENAI_GPT_5_6_TERRA: &str = "openai/gpt-5.6-terra";

pub const DEFAULT_MODEL: &str = DEFAULT_ROUTING;

/// Curated routes shown in VT Code's model picker. Merge Gateway also accepts
/// other valid `provider/model` identifiers through explicit configuration.
pub const SUPPORTED_MODELS: &[&str] = &[
    DEFAULT_ROUTING,
    OPENAI_GPT_5_5,
    ANTHROPIC_CLAUDE_OPUS_5,
    GOOGLE_GEMINI_3_6_FLASH,
    GOOGLE_GEMINI_3_7_FLASH,
    DEEPSEEK_V4_PRO_0813,
    DEEPSEEK_V4_FLASH_0731,
    XAI_GROK_4_6,
    QWEN_3_8_MAX,
    MINIMAX_H3,
    MOONSHOT_KIMI_K3,
    THINKINGMACHINES_INKLING,
    META_MUSE_SPARK_1_1,
    OPENAI_GPT_5_6_LUNA,
    OPENAI_GPT_5_6_SOL,
    OPENAI_GPT_5_6_TERRA,
];

/// Merge controls reasoning per route, so VT Code does not forward a
/// provider-wide reasoning effort parameter for these models.
pub const REASONING_MODELS: &[&str] = &[];
