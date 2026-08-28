# TODO

## Memory fixes

- [ ] Investigate and fix the memory feature not saving user context.
    - **Observed:** After asking VT Code to remember that the user is Vinh Nguyen / `vinhnx`, the assistant reported: “Couldn't save memory because the LLM planner still needs more information.”
    - **Expected:** A sufficiently specific, user-approved fact should be persisted to the session-independent memory store and be available in later turns.
    - **Reproduction context:** The request followed a conversation in which `vinhnx` was identified from local repository metadata and public profiles. The save attempt failed before any confirmation that a memory file or durable store entry was created.
    - **Acceptance criteria:** - Saving a clear user preference or identity alias does not require unrelated planner information. - The user receives an actionable error when persistence fails, including what additional information is required. - A successful save is verified by reading the memory through the supported memory path in a subsequent turn. - Add regression coverage for the planner/memory-save path and the failure message above.
      log: local `.vtcode/checkpoints/` artifacts (`turn_995.json` through `turn_991.json`)

session: local `.vtcode/sessions/` artifact for the reproduction

---

CRITICAL: check vtcode post-amble summarized session is sometimes gone/missing. it was working before. context: when user control+c or quit the program, there is the summarization turn/context shown in the CLI. Currently it showing a blank space. This is a regression from the previous behavior. The summarization turn/context should be shown in the CLI after the user quits the program.

===

## WebMCP Challenge

References: [OpenAI challenge](https://openai.com/webmcp-challenge/), [Devpost rules and submission](https://webmcp.devpost.com/), [WebMCP specification](https://webmachinelearning.github.io/webmcp/).

### Immediate reminders

- [x] **REMINDER — Register:** Create or join the Devpost submission before implementation begins.
- [x] **REMINDER — Implement:** The WebMCP reference client and first-class VT Code bridge were added after August 25, 2026; the commit history preserves that work.
- [ ] **REMINDER — Submit:** Submit before September 3, 2026 at 1:00 PM PDT (September 4 at 3:00 AM ICT), then freeze the submitted repository, deployment, and video.

### Product direction

The WebMCP reference client demonstrates VT Code's safe human-agent coding workflow. It uses an embedded sample project so the live client needs no credentials or backend access, while the authenticated bridge remains available for a real VT Code session.

The agent should inspect files, search code, propose a reversible patch, show the diff, wait for human approval, run deterministic checks, and record an audit entry. The app should make the collaboration visible rather than presenting an autonomous editor.

Choose the public project name manually; do not treat a generated name as final.

### WebMCP tool contract

| Tool                   | Behavior                         | Safety posture                     |
| ---------------------- | -------------------------------- | ---------------------------------- |
| `list_project_files`   | List the embedded project tree   | Read-only                          |
| `search_code`          | Search bounded file contents     | Read-only                          |
| `read_file`            | Read a bounded file segment      | Read-only                          |
| `get_editor_state`     | Inspect editor workflow state    | Read-only                          |
| `open_file`            | Open a selected file             | Browser UI state                   |
| `stage_text_edit`      | Stage one exact draft replacement | Browser draft only                 |
| `review_draft`         | Show the current draft diff      | Read-only preview                  |
| `open_panel`           | Navigate editor panels           | Browser UI state                   |

Approval, apply, check, and revert remain UI or bridge operations outside the
browser WebMCP tool set; the browser agent cannot authorize a filesystem write.

### Implementation phases

1. **Scaffold and deploy**
    - Keep the reference browser client isolated under `examples/webmcp-challenge` and ship the authenticated bridge in the main Rust workspace.
    - Add a visible open-source license, setup instructions, and a static deployment.
    - Register one read-only WebMCP tool and verify it in ChatGPT's in-app browser and Chrome with WebMCP enabled.

2. **Build the shared workspace**
    - Add the embedded sample project, file tree, search, file viewer, diff viewer, validation panel, snapshot store, and audit timeline.
    - Keep tool descriptions, parameter descriptions, and outputs short and deterministic.

3. **Add human approval and dynamic tools**
    - Make `stage_text_edit` produce a staged browser draft only.
    - Keep approval, application, checks, and revert outside the browser WebMCP tool set.
    - Unregister the browser registration through its `AbortSignal` and observe `toolchange` notifications.
    - Provide clear failure responses for invalid paths, malformed patches, stale snapshots, and failed checks.

4. **Harden and evaluate**
    - Validate arguments at runtime instead of trusting JSON Schema alone.
    - Bound file reads, diff size, tool outputs, and check duration.
    - Mark read-only and untrusted-content tools appropriately.
    - Test tool discovery, `toolchange`, approval gating, cancellation, navigation, revert, and the no-WebMCP fallback.
    - Use a Playwright browser-reviewer agent for repeatable smoke tests.

5. **Prepare the submission**
    - Write a README explaining why WebMCP is necessary, what humans and agents can do together, and how the tools are implemented.
    - Add a one-command local run path and live URL testing instructions.
    - Record a public demo video shorter than three minutes with audio:
        1. Show the human problem and workspace.
        2. Ask the agent to investigate and propose a fix.
        3. Show the staged diff and human approval.
        4. Run checks and show the audit trail.
        5. Explain the WebMCP-specific improvement.
    - Draft the Devpost description around usefulness, WebMCP leverage, execution, impact, and creativity.

### Timebox

- [ ] Aug 26: register, choose scope, preserve baseline, and create the companion repository or branch.
- [ ] Aug 27: scaffold, deploy, and prove one WebMCP read-only tool works.
- [ ] Aug 28–29: implement project browsing, search, patch proposal, diff, and checks.
- [ ] Aug 30: implement approval gating, dynamic registration, snapshots, revert, audit, and security limits.
- [ ] Aug 31: run ChatGPT/Chrome tests, attend office hours if useful, and fix the highest-impact issues.
- [ ] Sep 1: polish the product flow, README, and failure states.
- [ ] Sep 2: record the video, complete the Devpost draft, and run the final smoke test.
- [ ] Sep 3: submit before the deadline and stop editing the submitted artifacts.

### Scope guardrails

- Do not build a generic WebMCP-to-MCP proxy for this challenge.
- Do not connect the live reference client to an unbounded repository or allow the agent to approve its own mutations.
- Do not make the challenge depend on an LLM API key; the browser agent should be enough to demonstrate the workflow.
- Keep challenge-specific browser assets isolated, document the first-class bridge clearly, and avoid unrelated repository changes.

===

implement intelligent container width for display table/blocks

if width is enough use the wide table-layout regression screenshot
if not use the narrow heading-block regression screenshot

===

implement /secret filter

===

on /config, revise the UI and when go back from section -> keep previous selected entry. also revamp the /config UI to be more user friendly and intuitive. implement live reload of config changes without needing to restart the program. implement a /config reset command to reset all config to default values. Both in TUI and CLI.
