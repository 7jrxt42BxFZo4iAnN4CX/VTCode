# Planning Workflow

The planning workflow lets you iterate with the agent on what you want to build before implementation starts. It is driven by the built-in `plan` primary agent and the `/plan` slash command.

## Overview

During planning, the agent can:

- read files and inspect project structure
- search code with grep, structural search, and other read-only tools
- analyse patterns and constraints before proposing changes
- run explicitly safe inspection or validation commands when the active permission policy allows them

Shell commands in plan mode are validated against a read-only allow-list. Allowed patterns include:

- inspection base commands: `rg`, `ls`, `cat`, `sed`, `grep`, `find`, `head`, `tail`, `fd`, `tree`, `stat`, `file`, `which`, `jq`, and similar
- `cd` prefixes: `cd <dir> && <read-only command>` (changing directory mutates nothing)
- read-only subcommands: `git status|log|diff|show|blame|ls-files|rev-parse|describe|shortlog|grep`, `cargo check|test|clippy|metadata|tree|nextest run`, `npm|pnpm|yarn test`
- `&&` chains and `|` pipelines where every segment is itself read-only
- `2>&1` stderr merges (no file is written)

Rejected: file redirections (`>`, `>>`), command substitution (`$(...)`, backticks), `;` chaining, in-place edits (`sed -i`), and any segment starting with a mutating or unknown command (`rm`, `mv`, `cargo build`, `git push`, arbitrary scripts).

During planning, mutating tools should be denied unless a project explicitly allows durable planning artefacts such as files under `.vtcode/plans/`.

`task_tracker` is available for checklist state. Planning output should use `<proposed_plan>...</proposed_plan>` when the agent is ready for user review.

## Usage

### Start With The Planning Agent

Set the default primary agent to `plan` when you want new sessions to start with the built-in planning agent:

```toml
default_primary_agent = "plan"
```

You can also press `Tab` on an empty idle composer to cycle to the `plan` primary agent.

### Use `/plan`

`/plan` starts or continues the planning workflow. It is a workflow command, not a session state selector.

While a turn is actively processing, `/plan` is dropped with a notice (mode switches are locked for the duration of a turn). The automatic in-turn planning intent detection still engages on its own; only explicit `/plan` entry while busy is deferred.

```text
/plan
```

### Enter From an Agent Suggestion

The agent can also propose entering the planning workflow on its own when it
judges edits should be planned first. Under an interactive policy a HITL
confirmation prompt appears:

```text
Enter Planning workflow?

- Enter Planning workflow (Recommended) — enter planning and persist the draft under .vtcode/plans
- Continue without Planning workflow
```

- **Enter Planning workflow** — starts planning; the draft is persisted under
  `.vtcode/plans/` and mutating tools stay disabled until you approve execution.
- **Continue without Planning workflow** — the agent proceeds without planning
  (mutating tools remain enabled).

This gate prevents the agent from silently switching into plan mode; you decide
whether to plan before any edits begin. Full-auto and skip-confirmations
policies accept the suggestion directly, including in an interactive UI.

Execution agents such as `build`, `auto`, and `duck` can invoke the
`start_planning` tool when a request is demanding, ambiguous, or has multiple
phases. The tool only presents the entry prompt; it does not silently change
mode. Straightforward requests continue directly in the active execution
agent. In a headless session without an automatic execution policy, the
suggestion is reported as pending and the turn stops safely; use `/plan` on the
next turn to confirm entry. Full-auto or skip-confirmations policies may accept
the suggestion automatically.

### Intent Phrases

You can steer the workflow with short phrases instead of the review-gate UI:

- To **exit planning and present the plan** for approval, type `implement`,
  `approve`, `lgtm`, `ship it`, `yes`/`continue`/`go`/`start`, or select
  **Execute** / **Auto-accept** in the review gate. The plan is shown in an
  inline confirmation overlay (or a text prompt in non-interactive mode);
  the agent will not self-approve by editing the plan file and staying in
  plan mode.
- To **stay in planning**, type `stay in planning` (or revise the
  `<proposed_plan>` block). This overrides any exit phrase.
- To **cancel planning without implementation**, type `no`, `cancel`, or
  `abandon plan`, use `/plan off`, or switch away from the `plan` agent. VT
  Code emits a terminal `cancel` approval event and removes the active plan
  draft when the workflow is explicitly abandoned.

### Typical Workflow

1. Select the `plan` primary agent or run `/plan`.
2. Describe the goal and constraints.
3. Iterate on repository facts, risks, and open decisions.
4. Review the emitted `<proposed_plan>` block.
5. Switch to a build-oriented primary agent such as `build` or `auto` when you are ready to implement.

When planning was entered from another primary agent, approving the plan
restores that agent automatically when it is write-capable. `build` resumes
with reviewable edits and `auto` resumes its configured automation policy.
Read-only agents such as `duck` and `plan` are never selected to execute an
approved plan; the handoff resolves to the configured write-capable agent or
the built-in `build` agent. If the dedicated `plan` agent was already active
when planning began, approval uses the configured default execution agent
only when that agent can mutate the workspace.

### Clarification Interviews

When the planning agent reaches a material ambiguity, or identifies an open
decision before presenting a plan, it asks through `request_user_input`. In an
interactive session this opens an inline wizard with a selectable option list
(and an optional free-form note). The selected answer is returned to the same
planning turn, recorded as the completed interview, and supplied to the next
model request so planning can continue with the user's choice.

Pressing `Esc` or `Ctrl-C` cancels the interview without submitting an answer;
the planning agent may ask again when the ambiguity still matters. A plan
approval popup is not shown until any required clarification has completed.

## Plan Output Format

Planning output should stay decision-complete but sparse — treat it like a
compact spec, not prose. Keep the whole `<proposed_plan>` under ~1500 tokens;
prefer `file:symbol` references over narrative. This bound exists because an
overly verbose plan is truncated at the model's output-token limit (cut off
mid-plan) and must then be condensed and re-emitted.

File references must be plain text or inline code, never markdown links or
editor/IDE URIs — plans are read in terminals and other non-hyperlink
surfaces:

```markdown
Correct: `src/main.rs:42` or src/main.rs:42
Incorrect: [main.rs:42](vscode-file://vscode-app/.../workbench.html)
Incorrect: [main.rs](file:///Users/you/repo/src/main.rs#L42)
```

```markdown
Repository facts checked:

- [file:symbol or behaviour confirmed from the repo]

Next open decision: [if any], otherwise: No remaining scope decisions.

<proposed_plan>

# [Task Title]

## Summary

[1-3 lines: goal, user impact, what changes / what does not]

## Steps

1. [Action] -> files/symbols -> verify: [check]
2. [Action] -> files/symbols -> verify: [check]

## Validation

- build/lint: [detected toolchain command]
- tests: [detected toolchain command]
- behaviour: [targeted check]

## Assumptions

- [assumption or default chosen]
- [out-of-scope item intentionally not changed]
  </proposed_plan>
```

Only `Next open decision` is used as the explicit reopen marker for follow-up planning.

### Research Scope

Research effort should scale with the request. For a narrow or simple ask,
a handful of targeted reads/searches (roughly 5-10) is usually enough before
drafting `<proposed_plan>` — exhaustively enumerating the whole repository
for a simple request wastes the turn's tool-call and wall-clock budget and
can exhaust it before a plan is produced. For a broad or ambiguous ask,
research proportionally more, but stop and draft as soon as the
scope/decomposition/verification decisions are closed.

## Review Gate

After a plan is ready, an interactive human-in-the-loop (HITL) confirmation popup presents a structured summary (phases/steps
checklist, or the raw plan when structured data is absent) and a decision gate.

Approval options:

- **Approve (Auto-accept edits)** — approve and execute the plan automatically (Recommended).
- **Approve (Manual review)** — approve and execute the plan with per-step edit approvals.
- **Edit Plan** — open the plan file (`.vtcode/plans/...`) in your editor to make direct changes (or press `ctrl-g`).
- **Discuss & Revise** — return to chat to provide feedback, ask clarifying questions, or revise the plan with the agent.
- **Switch to build agent** — hand off execution to the `build` primary agent (manual per-step edit confirmations).
- **Switch to auto agent** — hand off execution to the `auto` primary agent (auto-execute).

Handoff options perform a true primary-agent switch: the chosen agent becomes active and
executes the approved plan. The planning workflow is disabled and mutating tools are enabled
once the user approves the plan.

The approved execution turn ends with a concise summary of the outcome, changed
files, verification performed, and remaining blockers. In interactive sessions
this appears alongside the final response. In headless sessions, a plan that
requires confirmation remains pending and the run stops safely after emitting
the plan; send `approve` or `implement` on a later turn to continue. Existing
full-auto or skip-confirmation policies may continue automatically without an
interactive prompt.

### Runtime Events

All clients can reconstruct the approval lifecycle from the authoritative
`ThreadEvent` stream. A plan turn emits `plan.delta` and the completed plan item,
then `plan.approval.requested` with the producing turn and plan file. The
terminal decision is emitted as `plan.approval.resolved` with one of
`execute`, `auto_accept`, `revise`, `cancel`, `switch_build`, or `switch_auto`,
plus an `automatic` flag. Interactive and headless clients therefore observe
the same pending, revision, and implementation handoff states. The Open
Responses bridge forwards these as `vtcode.plan_approval_requested` and
`vtcode.plan_approval_resolved` custom events.

## Budget Exhaustion

If the session budget or wall-clock limit is reached while planning, the
runloop does **not** force another interview or loop (no further LLM calls are
possible). Instead it preserves the current plan draft, reports the limit, and
leaves the planning session available for a text approval or revision command
on the next turn.

## Plan File Persistence

The draft is the single source of truth and always lives on disk under
`.vtcode/plans/<plan>.md`, not only in chat history. Even when the final
synthesis fails and the runloop enters the tool-free recovery pass (where all
tools — including `apply_patch` and `task_tracker` — are disabled), any
`<proposed_plan>` the model emitted inline is extracted and written to the
session plan file before the turn ends. This is why the budget/recovery
exhaustion notices can promise the draft is "preserved in the session plan
file": the recovery path persists it rather than leaving `.vtcode/plans/` at its
`start_planning` template. If you ever see an empty plan dir after planning,
treat it as a regression in this persistence path, not a permissions or
directory-creation problem.

## Best Practices

1. Be specific about files, functions, constraints, and desired behaviour.
2. Ask the agent to state trade-offs before implementation begins.
3. Keep the planning agent read-oriented and switch to `build`, `auto`, or `review` for the next phase.

## See Also

- [Command reference](../user-guide/commands.md)
- [Subagents and primary agents](../user-guide/subagents.md)
- [Configuration precedence](../config/CONFIGURATION_PRECEDENCE.md)
