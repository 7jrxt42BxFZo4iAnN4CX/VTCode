use crate::tools::command_args::{is_readonly_command_string, raw_command_text};
use serde_json::Value;

const TOOL_OUTPUT_SPOOL_DIRECTORY: &str = ".vtcode/context/tool_outputs";

/// Conservative allow-list of read-only inspection commands used by
/// `command_session`. Any command that could write, move, or delete must be
/// rejected so it is not cached or parallelized as read-only.
///
/// `cd` is included because changing the working directory mutates nothing;
/// models habitually prefix exploration with `cd <workspace> && …` and plan
/// mode must not reject that pattern (checkpoint turn_810).
const READONLY_UNIFIED_EXEC_COMMANDS: &[&str] = &[
    "rg", "ls", "cat", "diff", "find", "wc", "grep", "egrep", "fgrep", "head", "tail", "sort", "uniq", "awk", "sed",
    "cut", "tr", "ast-grep", "sg", "echo", "pwd", "printf", "true", "false", "test", "cd", "fd", "tree", "which",
    "stat", "file", "du", "df", "realpath", "basename", "dirname", "nl", "column", "jq", "date", "whoami", "uname",
];

pub fn is_readonly_base_command(command: &str) -> bool {
    READONLY_UNIFIED_EXEC_COMMANDS.contains(&command)
}

/// Read-only subcommand allow-list for multi-word tools whose base command is
/// not inherently safe (`git`, `cargo`, package managers). Only inspection
/// subcommands are listed; anything that can mutate the worktree, index,
/// refs, or lockfiles must stay out.
fn is_readonly_subcommand(first: &str, second: Option<&str>, third: Option<&str>) -> bool {
    match first {
        "git" => matches!(
            second,
            Some(
                "status"
                    | "log"
                    | "diff"
                    | "show"
                    | "blame"
                    | "ls-files"
                    | "rev-parse"
                    | "describe"
                    | "shortlog"
                    | "grep"
            )
        ),
        "cargo" => match second {
            Some("check" | "test" | "metadata" | "tree" | "clippy") => true,
            Some("nextest") => matches!(third, Some("run" | "list")),
            _ => false,
        },
        "npm" | "pnpm" | "yarn" => match second {
            Some("test") => true,
            Some("run") => matches!(third, Some("test")),
            _ => false,
        },
        _ => false,
    }
}

/// Leading command words of a shell segment, skipping flags and `VAR=value`
/// assignments. Bounded to the first three words, which is all the
/// subcommand allow-list needs (e.g. `cargo nextest run`).
fn segment_command_words(segment: &str) -> Vec<String> {
    segment
        .split_whitespace()
        .filter(|token| !token.starts_with('-') && !token.contains('='))
        .take(3)
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

/// A shell segment (one `&&`-chain element, possibly a `|` pipeline) is
/// read-only when every pipeline stage starts with an allow-listed base
/// command or an allow-listed read-only subcommand (`git log`, `cargo tree`).
fn is_readonly_segment(segment: &str) -> bool {
    let trimmed = segment.trim();
    if trimmed.is_empty() {
        return false;
    }
    trimmed.split('|').all(|sub| {
        let words = segment_command_words(sub);
        let Some(first) = words.first() else {
            return false;
        };
        is_readonly_base_command(first)
            || is_readonly_subcommand(first, words.get(1).map(String::as_str), words.get(2).map(String::as_str))
    })
}

pub fn is_readonly_command_session_command(args: &Value) -> bool {
    let Ok(Some(parts)) = crate::tools::command_args::command_words(args) else {
        return false;
    };

    if parts.iter().any(|part| part == "--dry-run") {
        return true;
    }

    let Some(raw) = raw_command_text(args) else {
        return false;
    };

    // Verify the raw command has no redirections, command substitutions, or
    // destructive subcommands (e.g. `find -delete`, `-exec rm`, `sed -i`).
    if !is_readonly_command_string(args) {
        return false;
    }

    // Every `&&`-chain segment — and every `|` pipeline stage within each
    // segment — must be an allow-listed read-only command. This permits
    // harmless exploration like `cd repo && git log --oneline | head -20`
    // while rejecting `ls && rm foo.txt` (checkpoints turn_726, turn_810).
    raw.split("&&").all(is_readonly_segment)
}

/// Returns `true` when a safe shell inspection command reads the internal tool
/// output spool directory. Such reads must stay inline: spooling their output
/// again would create a recursive chain of spool references.
pub fn is_spool_file_read_command(tool_name: &str, args: &Value) -> bool {
    if super::classify::canonical_command_session_tool_name(tool_name).is_none() {
        return false;
    }

    if !is_readonly_command_session_command(args) {
        return false;
    }

    raw_command_text(args).is_some_and(|command| command.replace('\\', "/").contains(TOOL_OUTPUT_SPOOL_DIRECTORY))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run_cmd(command: &str) -> Value {
        json!({"action": "run", "command": command})
    }

    #[test]
    fn and_chain_allows_readonly_segments() {
        assert!(is_readonly_command_session_command(&run_cmd("ls -la && echo '---' && ls -la crates/")));
        assert!(is_readonly_command_session_command(&run_cmd("pwd && ls src/")));
        assert!(is_readonly_command_session_command(&run_cmd("cat foo.txt && grep bar")));
    }

    #[test]
    fn and_chain_rejects_destructive_segments() {
        assert!(!is_readonly_command_session_command(&run_cmd("ls -la && rm foo.txt")));
        assert!(!is_readonly_command_session_command(&run_cmd("cat x && mv a b")));
        assert!(!is_readonly_command_session_command(&run_cmd("true && cp a b")));
    }

    #[test]
    fn and_chain_rejects_non_allowlisted_segments() {
        assert!(!is_readonly_command_session_command(&run_cmd("ls -la && python script.py")));
        assert!(!is_readonly_command_session_command(&run_cmd("ls -la && cargo build")));
    }

    #[test]
    fn and_chain_allows_pipeline_within_segment() {
        assert!(is_readonly_command_session_command(&run_cmd("ls -la | head -5 && echo done")));
    }

    #[test]
    fn and_chain_single_command_passes() {
        assert!(is_readonly_command_session_command(&run_cmd("ls -la")));
        assert!(is_readonly_command_session_command(&run_cmd("echo hi")));
    }

    #[test]
    fn readonly_command_session_allows_and_chain() {
        // The exact pattern from checkpoint turn_726 that was blocked.
        assert!(is_readonly_command_session_command(&run_cmd("ls -la /path/ && echo '---' && ls -la /path/crates/")));
    }

    #[test]
    fn readonly_command_session_rejects_destructive_and_chain() {
        assert!(!is_readonly_command_session_command(&run_cmd("ls -la && rm foo.txt")));
    }

    #[test]
    fn cd_prefixed_exploration_is_readonly() {
        // Exact patterns from checkpoint turn_810 that plan mode rejected:
        // the model habitually prefixes exploration with `cd <workspace> &&`.
        assert!(is_readonly_command_session_command(&run_cmd("cd /repo && sed -n '440,520p' Cargo.toml")));
        assert!(is_readonly_command_session_command(&run_cmd(
            "cd /repo && ls -la && echo '---' && rg --files -g 'Cargo.toml' | sed -n '1,120p'"
        )));
        // `cd` must not become a bypass for mutating chains.
        assert!(!is_readonly_command_session_command(&run_cmd("cd /repo && cargo build")));
        assert!(!is_readonly_command_session_command(&run_cmd("cd /repo && rm -rf target")));
    }

    #[test]
    fn git_readonly_subcommands_allowed_in_chains_and_pipelines() {
        assert!(is_readonly_command_session_command(&run_cmd("git log --oneline | head -20")));
        assert!(is_readonly_command_session_command(&run_cmd("cd /repo && git diff")));
        assert!(is_readonly_command_session_command(&run_cmd("git show HEAD")));
        assert!(is_readonly_command_session_command(&run_cmd("git blame src/main.rs | head")));
        // Mutating git subcommands stay rejected.
        assert!(!is_readonly_command_session_command(&run_cmd("git checkout main")));
        assert!(!is_readonly_command_session_command(&run_cmd("cd /repo && git push")));
        assert!(!is_readonly_command_session_command(&run_cmd("git commit -m 'x'")));
    }

    #[test]
    fn cargo_readonly_subcommands_allowed() {
        assert!(is_readonly_command_session_command(&run_cmd("cargo metadata")));
        assert!(is_readonly_command_session_command(&run_cmd("cargo tree | head -50")));
        assert!(is_readonly_command_session_command(&run_cmd("cargo nextest run")));
        assert!(!is_readonly_command_session_command(&run_cmd("cargo build")));
        assert!(!is_readonly_command_session_command(&run_cmd("cargo publish")));
    }

    #[test]
    fn stderr_merge_is_not_a_write_redirection() {
        assert!(is_readonly_command_session_command(&run_cmd("cargo check 2>&1 | head -c 4000")));
        // A real file redirection is still rejected.
        assert!(!is_readonly_command_session_command(&run_cmd("cargo check > out.txt")));
    }

    #[test]
    fn extra_inspection_base_commands_are_readonly() {
        assert!(is_readonly_command_session_command(&run_cmd("tree -L 2 crates/")));
        assert!(is_readonly_command_session_command(&run_cmd("fd -e rs planner")));
        assert!(is_readonly_command_session_command(&run_cmd("stat Cargo.toml && file target")));
        assert!(is_readonly_command_session_command(&run_cmd("which cargo")));
    }

    #[test]
    fn spool_file_reads_are_detected_only_for_readonly_commands() {
        for command in [
            "cat .vtcode/context/tool_outputs/run-1.txt",
            "sed -n '1,20p' .vtcode/context/tool_outputs/run-1.txt",
            "rg error .vtcode/context/tool_outputs",
        ] {
            assert!(is_spool_file_read_command("exec_command", &run_cmd(command)), "expected spool read: {command}");
        }

        for command in [
            "cat .vtcode/context/tool_outputs/run-1.txt > copied.txt",
            "sed -i 's/a/b/' .vtcode/context/tool_outputs/run-1.txt",
            "rm .vtcode/context/tool_outputs/run-1.txt",
        ] {
            assert!(!is_spool_file_read_command("exec_command", &run_cmd(command)), "unexpected spool read: {command}");
        }

        assert!(!is_spool_file_read_command("mcp_tool", &run_cmd("cat .vtcode/context/tool_outputs/run-1.txt")));
        assert!(is_spool_file_read_command("exec_command", &run_cmd("cat .vtcode/context/tool_outputs/run-1.txt")));
    }
}
