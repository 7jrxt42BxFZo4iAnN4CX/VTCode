#![allow(
    missing_docs,
    clippy::expect_used,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize, Clone)]
#[allow(
    dead_code,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
pub struct PermissionFixture {
    #[serde(rename = "sessionId")]
    pub session_id: Value,
    #[serde(rename = "toolCall")]
    pub tool_call: Value,
    pub arguments: Value,
}

pub fn read_file_permission() -> PermissionFixture {
    load_fixture(include_str!("fixtures/acp/permission_read_file.json"))
}

pub fn list_files_permission() -> PermissionFixture {
    load_fixture(include_str!("fixtures/acp/permission_list_files.json"))
}

fn load_fixture(contents: &str) -> PermissionFixture {
    serde_json::from_str(contents).expect("invalid ACP permission fixture")
}
