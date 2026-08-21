#![allow(
    missing_docs,
    reason = "Integration tests exercise apply_patch containment through its public API."
)]

use tempfile::TempDir;
use vtcode_core::tools::editing::Patch;

#[test]
fn parser_rejects_absolute_and_lexically_traversing_patch_paths() {
    for patch_text in [
        "*** Begin Patch\n*** Add File: /tmp/escaped.txt\n+escaped\n*** End Patch\n",
        "*** Begin Patch\n*** Add File: ../escaped.txt\n+escaped\n*** End Patch\n",
    ] {
        assert!(Patch::parse(patch_text).is_err(), "parser accepted {patch_text:?}");
    }
}

#[tokio::test]
async fn applies_a_valid_workspace_relative_path() {
    let workspace = TempDir::new().expect("workspace should be created");
    let patch = Patch::parse("*** Begin Patch\n*** Add File: src/created.txt\n+safe\n*** End Patch\n")
        .expect("valid patch should parse");

    let _ = patch
        .apply(workspace.path())
        .await
        .expect("workspace-relative patch should apply");

    let content = tokio::fs::read_to_string(workspace.path().join("src/created.txt"))
        .await
        .expect("created file should be readable");
    assert_eq!(content, "safe\n");
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_escape_is_rejected_before_any_patch_mutation() {
    use std::os::unix::fs::symlink;

    let workspace = TempDir::new().expect("workspace should be created");
    let outside = TempDir::new().expect("outside directory should be created");
    let outside_file = outside.path().join("victim.txt");
    tokio::fs::write(&outside_file, "outside\n")
        .await
        .expect("outside file should be written");
    symlink(outside.path(), workspace.path().join("escape")).expect("symlink should be created");

    let patch = Patch::parse(
        "*** Begin Patch\n*** Add File: created.txt\n+must not exist\n*** Delete File: escape/victim.txt\n*** End Patch\n",
    )
    .expect("patch with workspace-relative paths should parse");

    let error = patch
        .apply(workspace.path())
        .await
        .expect_err("symlink escape must be rejected");

    assert!(error.to_string().contains("escapes the workspace root via symlink"));
    assert!(!workspace.path().join("created.txt").exists(), "preflight must reject before creating files");
    let content = tokio::fs::read_to_string(&outside_file)
        .await
        .expect("outside file should remain readable");
    assert_eq!(content, "outside\n");
}
