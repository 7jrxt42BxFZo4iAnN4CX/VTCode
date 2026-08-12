//! Shell activity classification for progress accounting and output previews.
//!
//! Mutation safety remains owned by [`super::classify_tool_intent`]. This
//! module adds the narrower distinction between repository inspection and
//! verification without duplicating that safety decision in binary consumers.

use std::path::Path;

use serde_json::Value;

/// Progress semantics for a command invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellActivity {
    /// Read-only repository or environment inspection.
    Inspection,
    /// A build, test, lint, or compile command that verifies work.
    Verification,
    /// A command that may mutate state and is not primarily verification.
    Mutation,
}

fn is_verification_invocation(words: &[String]) -> bool {
    let mut words = words
        .iter()
        .map(String::as_str)
        .filter(|word| !word.contains('=') && *word != "env");
    let first = words.next().unwrap_or_default();
    let second = words.next().map(str::to_ascii_lowercase);
    let third = words.next().map(str::to_ascii_lowercase);
    let program = Path::new(first)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(first)
        .to_ascii_lowercase();

    match program.as_str() {
        "cargo" => {
            matches!(second.as_deref(), Some("check" | "build" | "clippy" | "test"))
                || (second.as_deref() == Some("nextest") && third.as_deref() == Some("run"))
        }
        "go" => matches!(second.as_deref(), Some("test" | "build")),
        "npm" | "pnpm" | "yarn" => {
            matches!(second.as_deref(), Some("test" | "build"))
                || (second.as_deref() == Some("run") && matches!(third.as_deref(), Some("test" | "build")))
        }
        "rustc" | "pytest" | "xcodebuild" | "gradle" | "gradlew" => true,
        _ if first.ends_with("/scripts/check.sh") || first.ends_with("/scripts/check-dev.sh") => true,
        _ => false,
    }
}

fn contains_verification_invocation(command: &str) -> bool {
    command.split(['&', '|', ';']).any(|segment| {
        shell_words::split(segment)
            .ok()
            .is_some_and(|words| is_verification_invocation(&words))
    })
}

fn has_logical_sequencing(words: &[String]) -> bool {
    words.iter().any(|word| matches!(word.as_str(), "&&" | "||" | ";"))
}

fn is_known_inspection(words: &[String]) -> bool {
    let Some(program) = words
        .first()
        .and_then(|word| Path::new(word).file_name())
        .and_then(|name| name.to_str())
    else {
        return false;
    };
    if words
        .iter()
        .any(|word| matches!(word.as_str(), ">" | ">>" | "|" | "&&" | ";" | "||"))
    {
        return false;
    }

    let program = program.to_ascii_lowercase();
    match program.as_str() {
        "rg" | "grep" | "cat" | "head" | "tail" | "bat" | "ls" | "pwd" | "wc" => true,
        "sed" => !words.iter().any(|word| word == "-i" || word.starts_with("--in-place")),
        "find" => !words
            .iter()
            .any(|word| matches!(word.as_str(), "-delete" | "-exec" | "-execdir" | "-ok")),
        _ => false,
    }
}

/// Classify a shell call without weakening the authoritative mutation guard.
///
/// Output plumbing such as `2>&1`, `> build.log`, or `| head` does not turn a
/// primary verification command into a mutation for progress accounting.
#[must_use]
pub fn classify_shell_activity(tool_name: &str, args: &Value) -> ShellActivity {
    let intent = super::classify_tool_intent(tool_name, args);
    let command = crate::tools::command_args::raw_command_text(args);
    let words = crate::tools::command_args::command_words(args).ok().flatten();

    if words.as_deref().is_some_and(is_known_inspection) {
        return ShellActivity::Inspection;
    }

    let starts_with_verification = words.as_deref().is_some_and(is_verification_invocation);
    let contains_verification =
        starts_with_verification || command.as_deref().is_some_and(contains_verification_invocation);
    if !intent.mutating {
        return if contains_verification {
            ShellActivity::Verification
        } else {
            ShellActivity::Inspection
        };
    }

    if starts_with_verification && !words.as_deref().is_some_and(has_logical_sequencing) {
        ShellActivity::Verification
    } else {
        ShellActivity::Mutation
    }
}
