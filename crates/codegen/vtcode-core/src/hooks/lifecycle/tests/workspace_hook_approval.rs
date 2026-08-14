#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use super::*;
use std::path::Path;
use tempfile::TempDir;

use crate::config::{
    HookCommandConfig, HookGroupConfig, HooksConfig, LifecycleHooksConfig, WorkspaceHookCommand,
    WorkspaceLifecycleHooks,
};

/// Build a workspace-controlled hook snapshot for the given (event, command)
/// pairs. Mirrors what `ConfigManager::collect_workspace_lifecycle_hooks`
/// produces for a workspace `vtcode.toml`.
fn workspace_hooks(pairs: &[(&str, &str)]) -> WorkspaceLifecycleHooks {
    WorkspaceLifecycleHooks {
        commands: pairs
            .iter()
            .map(|(event, command)| WorkspaceHookCommand {
                event: (*event).to_string(),
                matcher: None,
                command: (*command).to_string(),
                timeout_seconds: None,
            })
            .collect(),
    }
}

fn session_start_config(commands: &[&str]) -> HooksConfig {
    HooksConfig {
        lifecycle: LifecycleHooksConfig {
            session_start: commands
                .iter()
                .map(|command| HookGroupConfig {
                    matcher: None,
                    hooks: vec![HookCommandConfig {
                        kind: Default::default(),
                        command: (*command).to_string(),
                        timeout_seconds: None,
                    }],
                })
                .collect(),
            ..Default::default()
        },
    }
}

fn build_engine(
    workspace: &Path,
    config: &HooksConfig,
    hooks: Option<&WorkspaceLifecycleHooks>,
) -> LifecycleHookEngine {
    LifecycleHookEngine::new_with_session_and_workspace_hooks(
        workspace.to_path_buf(),
        config,
        SessionStartTrigger::Startup,
        "test-session",
        hooks,
    )
    .expect("engine constructs")
    .expect("non-empty hooks config produces an engine")
}

#[tokio::test]
async fn unapproved_workspace_hooks_are_skipped_fail_closed() {
    let temp_dir = TempDir::new().expect("tempdir");
    let workspace = temp_dir.path();
    let marker = workspace.join("vt-unapproved-marker");
    let command = format!("touch {}", marker.display());

    let config = session_start_config(&[&command]);
    let hooks = workspace_hooks(&[("session_start", &command)]);
    let engine = build_engine(workspace, &config, Some(&hooks));

    assert!(
        engine.workspace_hooks_need_approval().await,
        "workspace hooks must require approval before any approval is granted"
    );

    let outcome = engine.run_session_start().await.expect("run session start");

    assert!(!marker.exists(), "unapproved workspace lifecycle hook must not execute (GHSA-wqgw-crr5-cr2p)");
    assert!(
        outcome.messages.iter().any(|message| message.text.contains("not approved")),
        "skip reason should be surfaced, got: {:?}",
        outcome.messages
    );
}

#[tokio::test]
async fn approved_workspace_hooks_execute() {
    let temp_dir = TempDir::new().expect("tempdir");
    let workspace = temp_dir.path();
    let marker = workspace.join("vt-approved-marker");
    let command = format!("touch {}", marker.display());

    let config = session_start_config(&[&command]);
    let hooks = workspace_hooks(&[("session_start", &command)]);
    let engine = build_engine(workspace, &config, Some(&hooks));

    engine.approve_workspace_hooks(&hooks.digest()).await;

    assert!(!engine.workspace_hooks_need_approval().await, "matching digest approval must satisfy the gate");

    let outcome = engine.run_session_start().await.expect("run session start");
    assert!(marker.exists(), "approved workspace lifecycle hook should execute");
    assert!(
        !outcome.messages.iter().any(|message| message.text.contains("not approved")),
        "no skip messages expected after approval, got: {:?}",
        outcome.messages
    );
}

#[tokio::test]
async fn changed_workspace_commands_invalidate_prior_approval() {
    let temp_dir = TempDir::new().expect("tempdir");
    let workspace = temp_dir.path();
    let first_marker = workspace.join("vt-first-marker");
    let second_marker = workspace.join("vt-second-marker");
    let first_command = format!("touch {}", first_marker.display());
    let second_command = format!("touch {}", second_marker.display());

    // The user approved the original command set...
    let original = workspace_hooks(&[("session_start", &first_command)]);
    let first_digest = original.digest();

    // ...then a repository update replaced the command (new digest).
    let changed = workspace_hooks(&[("session_start", &second_command)]);
    assert_ne!(first_digest, changed.digest(), "digest must track command changes");

    let engine = build_engine(workspace, &session_start_config(&[&second_command]), Some(&changed));
    // Only the stale digest is recorded; the gate must refuse to honor it.
    engine.approve_workspace_hooks(&first_digest).await;

    engine.run_session_start().await.expect("run session start");

    assert!(
        !second_marker.exists(),
        "a command changed after approval must not execute under the stale approval"
    );
}

#[tokio::test]
async fn user_sourced_commands_still_run_without_workspace_approval() {
    let temp_dir = TempDir::new().expect("tempdir");
    let workspace = temp_dir.path();
    let user_marker = workspace.join("vt-user-marker");
    let workspace_marker = workspace.join("vt-workspace-marker");
    let user_command = format!("touch {}", user_marker.display());
    let workspace_command = format!("touch {}", workspace_marker.display());

    // The workspace set covers only the workspace-sourced command; the other
    // session-start hook comes from user-level configuration.
    let hooks = workspace_hooks(&[("session_start", &workspace_command)]);
    let engine = build_engine(workspace, &session_start_config(&[&user_command, &workspace_command]), Some(&hooks));

    engine.run_session_start().await.expect("run session start");

    assert!(user_marker.exists(), "user-sourced hooks must still run when workspace hooks are gated");
    assert!(!workspace_marker.exists(), "workspace-sourced hook must remain skipped while unapproved");
}

#[tokio::test]
async fn no_workspace_hooks_means_no_gate() {
    let temp_dir = TempDir::new().expect("tempdir");
    let workspace = temp_dir.path();
    let marker = workspace.join("vt-ungated-marker");
    let command = format!("touch {}", marker.display());

    // Engine built without workspace origin info (e.g. test/embedded callers):
    // nothing is gated because no workspace-controlled commands are known.
    let engine = build_engine(workspace, &session_start_config(&[&command]), None);

    assert!(!engine.workspace_hooks_need_approval().await);
    engine.run_session_start().await.expect("run session start");
    assert!(marker.exists(), "hooks without workspace origin must run as before");
}
