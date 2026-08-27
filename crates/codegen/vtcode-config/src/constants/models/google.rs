/// Default model - using stable version for production reliability
pub const DEFAULT_MODEL: &str = "gemini-3.7-flash";

pub const SUPPORTED_MODELS: &[&str] = &[
    "gemini-3.6-flash",           // Gemini 3.6 Flash - latest flash model with improved capabilities
    "gemini-3.7-flash",           // Gemini 3.7 Flash - latest flash model with 1M context and tunable thinking
    "gemini-3-flash-preview",     // Fast version of Gemini 3 Pro with 3-level thinking
    "gemini-3-pro-image-preview", // Image generation model with 4K resolution
];

/// Models that support thinking/reasoning capability with configurable thinking_level
/// Based on: <https://ai.google.dev/gemini-api/docs/gemini-3>
/// Gemini 3 Pro/Flash: supports low, high (default)
/// Gemini 3 Flash only: also supports minimal, medium
/// Gemini 3.7 Flash: supports low, medium (default), high
pub const REASONING_MODELS: &[&str] = &["gemini-3.6-flash", "gemini-3.7-flash", "gemini-3-flash-preview"];

/// Models that support Gemini 3 extended thinking levels (minimal, medium)
/// Only Gemini 3 Flash supports these additional levels beyond low/high
pub const EXTENDED_THINKING_MODELS: &[&str] = &["gemini-3-flash-preview"];

/// Models supporting image generation
pub const IMAGE_GENERATION_MODELS: &[&str] = &["gemini-3-pro-image-preview"];

/// Models that support context caching (min 2048 tokens required)
/// Context caching reduces costs for repeated API calls with similar contexts
/// Reference: <https://ai.google.dev/gemini-api/docs/caching>
pub const CACHING_MODELS: &[&str] = &["gemini-3.6-flash", "gemini-3.7-flash", "gemini-3-flash-preview"];

/// Models that support code execution (Python)
/// Code execution allows models to write and execute Python code
/// Reference: <https://ai.google.dev/gemini-api/docs/code-execution>
pub const CODE_EXECUTION_MODELS: &[&str] = &["gemini-3.6-flash", "gemini-3.7-flash", "gemini-3-flash-preview"];

// Convenience constants for commonly used models
pub const GEMINI_3_6_FLASH: &str = "gemini-3.6-flash";
pub const GEMINI_3_7_FLASH: &str = "gemini-3.7-flash";
pub const GEMINI_3_FLASH_PREVIEW: &str = "gemini-3-flash-preview";
pub const GEMINI_3_PRO_IMAGE_PREVIEW: &str = "gemini-3-pro-image-preview";
