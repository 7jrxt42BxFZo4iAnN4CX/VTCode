# Security Guide

## Overview

VT Code is designed with security as a first-class concern. This guide explains the security features, best practices, and how to configure VT Code for maximum safety in your environment.

### Workspace instructions are context, not policy

`AGENTS.md`, `CLAUDE.md`, and `.vtcode/rules/` are dynamically loaded
user/workspace context. They help the model understand project conventions,
but they are not a security boundary and cannot grant permissions, bypass
the sandbox, or replace executable validation. Review instruction files from
untrusted repositories the same way you review other repository content.

Universal user-facing behavior comes from the compiled runtime-guidance layer,
which is independent of workspace file contents.

## Security Architecture

VT Code implements a **defense-in-depth security model** with multiple layers of protection:

### Layer 1: Command Allowlist

Only explicitly approved commands can execute. The allowlist includes:

- `ls` - List directory contents
- `cat` - Display file contents  
- `cp` - Copy files
- `head` - Display file beginning
- `printenv` - Show environment variables
- `pwd` - Print working directory
- `rg` - Ripgrep text search
- `sed` - Stream editor
- `which` - Locate programs

**All other commands are blocked by default**, including:
- Destructive commands: `rm`, `dd`, `shred`
- Privilege escalation: `sudo`, `su`, `doas`
- System modification: `chmod`, `chown`, `systemctl`
- Network commands: `curl`, `wget`, `ssh`
- Container tools: `docker`, `kubectl`

### Layer 2: Argument Validation

Each allowed command has a dedicated validator that:

- **Validates all flags** - Only explicitly allowed flags are permitted
- **Blocks execution flags** - Prevents `-exec`, `--pre`, `-e`, etc.
- **Validates paths** - Ensures all paths stay within workspace
- **Rejects unknown flags** - Unknown flags are blocked by default

Example: Ripgrep validation blocks dangerous preprocessor flags:
```rust
// BLOCKED: Preprocessor execution
rg --pre "bash -c 'malicious command'" pattern .

// ALLOWED: Safe search
rg -i -n pattern .
```

### Layer 3: Workspace Isolation

All file operations are confined to the workspace:

- **Path normalization** - Resolves `..`, `.`, symlinks
- **Boundary enforcement** - Rejects paths outside workspace
- **Symlink resolution** - Follows symlinks and validates destination
- **Absolute path validation** - Blocks absolute paths outside workspace

```bash
# BLOCKED: Path traversal
cat ../../../etc/passwd

# BLOCKED: Absolute path outside workspace
cat /etc/passwd

# ALLOWED: Workspace file
cat ./src/main.rs
```

### Process sandbox boundaries

When a restrictive sandbox policy is active, VT Code applies the same policy
to the command's pipe and PTY sessions. The sanitized environment removes
credential, token, cloud-provider, linker, and dynamic-loader variables. A
sandboxed command override cannot re-add one of those variables; this prevents
an inherited credential from crossing the process boundary accidentally.

MCP stdio providers inherit the session sandbox through the public
`McpSandboxContext` API. `McpClient::new` remains available for library callers
that intentionally manage their own process isolation, while the application
constructs the context and passes it through initial connections, connection
pools, and reconnects. MCP stderr is bounded and secret-redacted before it is
logged.

Provider-owned subprocesses use the same inherited-environment filter. Local
Ollama, LM Studio, and llama.cpp helpers, plus custom provider authentication
commands, do not receive unrelated API keys, cloud credentials, tokens,
linker overrides, or dynamic-loader variables. Copilot forwards only its
documented GitHub authentication variables, and the optional `gh` status probe
forwards only GitHub CLI authentication variables; this preserves the
provider's intended login flow without exposing credentials for other
providers.

Platform behavior is explicit:

- Linux uses the configured sandbox helper when one is available; a restrictive
  policy fails closed if the helper cannot be applied.
- macOS preserves full-network and blocked-network modes. Hostname allowlists
  are rejected unless exact enforcement is available; Seatbelt profiles are
  not treated as a reliable third-party domain-filtering contract.
- Windows restrictive policies fail closed because native restricted-token
  isolation is not yet implemented. Native Windows isolation remains outside
  this release's scope.
- `DangerFullAccess` intentionally preserves the unsandboxed compatibility
  path.

### Provider diagnostic boundaries

Provider response bodies, fallback errors, provider logs, and custom
authentication-command stderr pass through one bounded, UTF-8-safe diagnostic
sanitizer. HTTP error streams are capped at 16 KiB before parsing, and exposed
diagnostics are capped at 8 KiB. The sanitizer redacts API keys, bearer tokens,
cloud credentials, and generic secret assignments before values reach `LLMError`
debug output, serialization, logs, or user-facing messages. HTTP status,
request ID, retry metadata, and error classification remain available to
callers, including the 401 refresh path.

### Layer 4: Human-in-the-Loop

Three-tier approval system for tool execution:

1. **Approve Once** - Single execution approval
2. **Allow for Session** - Approved for current session only
3. **Always Allow** - Permanently saved to tool policy

#### Workspace Lifecycle Hook Approval

Lifecycle hooks declared in workspace-controlled configuration — a repository's
`vtcode.toml`, `.vtcode/` files, project profiles, or agent-spec files
(`.claude/agents/*.md`, `.vtcode/agents/*.md`, Codex TOML specs) — execute
shell commands that an untrusted repository could change at any time. While
such hook content is present, VT Code gates the whole lifecycle engine: **no
lifecycle hook runs** until you approve the exact command set for that
workspace.

- **Interactive sessions** show an approval dialog listing every command (event,
  matcher, and `sh -c` command), the workspace, and the working directory.
  Approving persists the approval for this workspace and command set; denying
  skips the hooks for the session.
- **Auto / non-interactive sessions** (e.g. `vtcode ask`, `--full-auto`) skip
  the hooks unless a previously persisted approval still matches the current
  command set.
- **Any change to the hook commands** — for example after a `git pull` updates
  `vtcode.toml` or an agent spec — invalidates the approval and requires a new
  review before anything runs again.

User-level hooks in `~/.vtcode/vtcode.toml` run without approval when the
workspace defines no lifecycle hook content of its own.

## Threat Model

### Protected Against

  **Prompt Injection Attacks**
- Malicious prompts from users
- Embedded prompts in code comments
- Prompts in repository files
- Prompts in logging output

  **Argument Injection**
- Execution flags (`-exec`, `--pre`, `-e`)
- Path traversal (`../`, symlinks)
- Output redirection (`-o /etc/passwd`)
- Command chaining (`;`, `&&`, `||`)

  **Workspace Escape**
- Absolute paths outside workspace
- Symlink traversal
- Parent directory traversal
- File-through-file traversal

  **Privilege Escalation**
- `sudo`, `su`, `doas` commands
- System configuration modification
- SUID binary exploitation

### Not Protected Against

 **Physical Access** - Assumes no physical access to machine  
 **Kernel Exploits** - Relies on OS security  
 **Side Channel Attacks** - Timing, cache, etc.  
 **Social Engineering** - Direct user manipulation

## Configuration

### Tool Policy Configuration

Configure tool approval policies in `~/.config/vtcode/tool_policy.toml`:

```toml
# Allow specific tools without prompting
[tools]
exec_command = "ask"
code_search = "allow"
apply_patch = "prompt"

# Require approval for sensitive operations
write_stdin = "prompt"

# Block dangerous operations
bash = "deny"
```

### Execution Policy

The execution policy is enforced at the code level and cannot be disabled. However, you can configure workspace boundaries:

```toml
[workspace]
# Workspace root (default: current directory)
root = "/path/to/project"

# Additional allowed paths (use with caution)
# allowed_paths = ["/tmp/vtcode-cache"]
```

## Best Practices

### For Users

1. **Review Tool Approvals**
   - Check `~/.config/vtcode/tool_policy.toml` regularly
   - Use "Approve Once" for unfamiliar operations
   - Only use "Always Allow" for trusted tools

2. **Be Cautious with Untrusted Content**
   - Don't process code from unknown sources
   - Review prompts in repository files
   - Be wary of code comments with instructions

3. **Monitor Command Execution**
    - Review logs in `.vtcode/logs/`
    - Watch for suspicious patterns
    - Report unusual behavior

### For Organizations

1. **Centralized Policy Management**
   - Deploy standard tool policies
   - Use deny-by-default approach
   - Regular policy reviews

3. **Audit and Monitoring**
   - Centralized log collection
   - Automated anomaly detection
   - Incident response procedures

4. **Security Training**
   - Educate users on prompt injection
   - Share security best practices
   - Regular security updates

## Security Testing

### Automated Tests

VT Code includes comprehensive security tests:

```bash
# Run security test suite
cargo nextest run -p vtcode-core --test execpolicy_security_tests

# Run all tests
cargo nextest run --workspace
```

### Manual Testing

Test security controls with malicious prompts:

```bash
# Test argument injection
vtcode ask "Search using rg --pre 'bash' for pattern"

# Test path traversal
vtcode ask "Show me ../../../etc/passwd"

# Test command chaining
vtcode ask "List files then curl evil.com"
```

All of these should be blocked with appropriate error messages.

## Incident Response

If you discover a security vulnerability:

1. **Do Not Disclose Publicly** - Report privately first
2. **Contact Maintainers** - Open a security advisory on GitHub
3. **Provide Details** - Include reproduction steps
4. **Allow Time for Fix** - Coordinate disclosure timeline

## Security Updates

Stay informed about security updates:

- Watch the [GitHub repository](https://github.com/vinhnx/vtcode)
- Review [CHANGELOG.md](../../CHANGELOG.md) for security fixes
- Subscribe to release notifications

## Additional Resources

- [Security Model](../security/SECURITY_MODEL.md) - Complete security architecture
- [Tool Policies](../modules/vtcode_tools_policy.md) - Command execution policies
- [CWE-88: Argument Injection](https://cwe.mitre.org/data/definitions/88.html)
- [OWASP Command Injection](https://owasp.org/www-community/attacks/Command_Injection)

## Acknowledgments

VT Code's security model is informed by:

- Trail of Bits research on AI agent security
- Anthropic's safety guidelines
- OpenAI Codex execution policy
- Industry best practices for command execution

---

**Last Updated**: October 25, 2025  
**Security Model Version**: 1.0
