//! Cached, documentation-mode-specific projections of immutable tool entries.

use crate::config::ToolDocumentationMode;
use crate::tools::handlers::compact::{compact_parameters, compact_tool_description};
use crate::tools::handlers::session_tool_catalog::ToolCatalogEntry;
use serde::Serialize;
use serde_json::Value;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub(super) struct ToolEntryProjection {
    description: String,
    parameters: Value,
    serialized_token_estimate: usize,
}

impl ToolEntryProjection {
    pub(super) fn description(&self) -> &str {
        &self.description
    }

    pub(super) fn parameters(&self) -> &Value {
        &self.parameters
    }

    pub(super) fn serialized_token_estimate(&self) -> usize {
        self.serialized_token_estimate
    }
}

#[derive(Debug)]
pub(super) struct ToolProjectionCache {
    entries: Vec<[OnceLock<ToolEntryProjection>; 3]>,
}

impl ToolProjectionCache {
    pub(super) fn new(entry_count: usize) -> Self {
        Self {
            entries: (0..entry_count).map(|_| std::array::from_fn(|_| OnceLock::new())).collect(),
        }
    }

    pub(super) fn get_or_init(
        &self,
        entry_index: usize,
        entry: &ToolCatalogEntry,
        documentation_mode: ToolDocumentationMode,
    ) -> &ToolEntryProjection {
        self.entries[entry_index][documentation_mode_index(documentation_mode)].get_or_init(|| {
            let description =
                compact_tool_description(entry.description.as_str(), documentation_mode, entry.max_description_length);
            let parameters = compact_parameters(entry.parameters.clone(), documentation_mode);
            let serialized_token_estimate = serde_json::to_string(&SerializedToolSchema {
                name: entry.public_name.as_str(),
                description: description.as_str(),
                parameters: &parameters,
            })
            .map(|serialized| serialized.len() / 4)
            .unwrap_or(0);

            ToolEntryProjection { description, parameters, serialized_token_estimate }
        })
    }
}

#[derive(Debug, Serialize)]
struct SerializedToolSchema<'a> {
    name: &'a str,
    description: &'a str,
    parameters: &'a Value,
}

fn documentation_mode_index(documentation_mode: ToolDocumentationMode) -> usize {
    match documentation_mode {
        ToolDocumentationMode::Minimal => 0,
        ToolDocumentationMode::Progressive => 1,
        ToolDocumentationMode::Full => 2,
    }
}
