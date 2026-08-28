# Devpost submission draft — The WebMCP Challenge

### ⏳ Not submitted yet
Nothing has been sent to Devpost.

This is a working draft for the existing Devpost project `VT`. Personal form
answers, a public demo video, and a real browser-agent pass still need
participant confirmation before the live draft is updated or submitted.

## Submission metadata

- **Project title from the current Devpost draft:** `VT`
- **One-line summary:** A browser editor where WebMCP agents inspect a
  workspace, stage one digest-checked draft edit, and hand it to VT Code for
  human-reviewed approval.
- **Live URL:** <https://vinhnx.github.io/VTCode/>
- **Public code repository:** <https://github.com/vinhnx/VTCode>
- **Demo video:** TODO — add a public YouTube video shorter than three minutes,
  with clear narration or other audio.
- **Open-source license:** The repository includes the MIT OR Apache-2.0
  license at [`LICENSE`](LICENSE).

## Project description

### What it is

VT is an IDE-shaped browser workspace for safe
human-and-agent collaboration. A visitor can explore a small in-memory project
without credentials, open files in CodeMirror, edit a draft, inspect a unified
diff, and use the browser's WebMCP surface to ask an agent to perform the same
structured actions. An optional authenticated WebSocket bridge connects the
editor to a running VT Code session and a real workspace.

The browser deliberately does not become a filesystem authority. Browser tools
can inspect files, search code, open a file, stage one exact text replacement
in a clean browser draft, review the resulting diff, and navigate the UI. They
cannot approve, apply, or revert a filesystem change. VT Code and its terminal
approval policy remain the authority for connected changes.

### Why WebMCP is a strong fit

Most browser agents have to infer editor state from pixels, DOM labels, and
keyboard shortcuts. This demo exposes the editor's meaningful operations as
typed, discoverable tools through `document.modelContext.registerTool`. The
agent can receive a bounded file listing, search result, file digest, or diff,
then use that structured result as the next tool input. The person remains in
the editor and reviews the exact proposed change before anything can reach the
terminal.

This makes a previously awkward workflow—find the right file, make a precise
change, inspect the diff, and hand it to a coding session—legible to both the
human and the agent without granting the browser direct filesystem write
access.

### What people and agents do together

- A person opens the public editor and can use it normally even when WebMCP is
  unavailable.
- An agent lists project files, searches for a symbol or phrase, and opens the
  relevant file using returned paths.
- An agent reads the current file and its `sha256:` digest, then stages one
  exact replacement only when the draft is clean and the digest still matches.
- The person reviews the generated diff in the **CHANGES** panel and decides
  whether to approve it.
- In an active VT Code session, the person can attach the reviewed diff to a
  prompt and let VT Code apply its normal terminal/full-auto policy.

## WebMCP implementation

The browser registers eight tools:

| Tool | Purpose | Authority |
| --- | --- | --- |
| `list_project_files` | List bounded workspace metadata | Read-only |
| `read_file` | Read a bounded file preview and digest | Read-only |
| `search_code` | Search bounded text across files | Read-only |
| `get_editor_state` | Inspect open file, dirty state, and panel | Read-only |
| `open_file` | Open a file in the editor | Browser UI state |
| `stage_text_edit` | Apply one exact replacement to a clean browser draft | Browser draft only; review required |
| `review_draft` | Create/read the current unified diff | Read-only preview |
| `open_panel` | Navigate to terminal, changes, or VT Code output | Browser UI state |

Each tool has a concise description, a JSON Schema with required properties and
validation, a display title, abort-aware execution, and explicit annotations
for read-only and untrusted content. File, diff, and collection responses are
bounded to a 1,500-character serialized result budget and include truncation
metadata when an agent should narrow its request. The app listens for WebMCP
`toolchange` events and unregisters tools through an `AbortSignal`. The
`get_editor_state` tool reports a small workflow state and recommended next
tools, while invalid states return recovery-oriented errors instead of generic
failures.

`stage_text_edit` is intentionally narrow: it requires a current digest,
rejects stale or dirty drafts, requires exactly one match, and returns the base
and draft digests. It never approves a patch, writes a local file, or bypasses
the authenticated VT Code bridge.

## AI and Codex usage

OpenAI Codex was used to inspect the existing VT Code bridge and demo, apply the
WebMCP tool-contract and security improvements, add deterministic browser evals,
update user/developer documentation, run focused Rust and JavaScript checks,
and deploy the public GitHub Pages demo. The application itself does not bundle
or claim a particular model runner: model tool selection is evaluated through a
WebMCP-capable browser client.

## Key features

- CodeMirror 6 editor with tabs, syntax highlighting, line numbers, dirty
  drafts, and a unified diff review panel.
- Deterministic in-memory fallback so judges can try the product without a
  local VT Code installation.
- Authenticated, loopback-oriented WebSocket bridge for workspace inspection
  and active VT Code turns.
- Digest-checked patch proposals and fail-closed stale-file handling.
- Explicit separation between browser draft mutation and terminal filesystem
  authority.
- Bounded outputs and input validation designed for browser-agent context.
- Workflow-state hints and actionable recovery messages for failed or stale
  agent steps.
- Checked-in deterministic eval cases for direct, open-ended, multi-step, and
  failure journeys. Each case records its goal, initial state, boundaries,
  expected UI effects, success criteria, and recovery guidance.

## Architecture

The Vite app in `examples/webmcp-challenge/` contains the editor UI, fallback
backend, bridge client, and WebMCP registration. `src/webmcp.js` owns the
browser tool contracts and bounded result handling. `src/main.js` adapts those
tools to the editor state and bridge. The Rust `vtcode-webmcp` crate owns
loopback WebSocket pairing, origin checks, token authentication, bounded
workspace operations, digest validation, and event replay. The connected
browser sends structured requests; VT Code remains responsible for policy,
approval, and authoritative file changes.

## Testing and judge instructions

### Quick public path

1. Open <https://vinhnx.github.io/VTCode/>.
2. Close **Settings** if it opens automatically; fallback mode needs no
   credentials.
3. Use the editor normally, or use a WebMCP-capable client to run the journeys
   below.

### WebMCP browser path

Use Chrome 149 or newer with `chrome://flags/#enable-webmcp-testing` enabled,
or use ChatGPT's in-app browser if it exposes WebMCP. Inspect the registered
tools, then try:

1. “What files are available in this workspace?” →
   `list_project_files`.
2. “Find the file that defines the greeting and open it in the editor.” →
   `search_code`, then `open_file` using the returned path.
3. “Change Hello to Hi in the greeting and prepare the draft for my review.” →
   `read_file`, `stage_text_edit` with its returned digest, then
   `review_draft`.
4. “Show me the current draft diff, then open the changes panel.” →
   `review_draft`, then `open_panel` with `changes`.
5. Request a long file or broad search and confirm the response stays bounded
   and reports truncation.

The repository's deterministic checks are run from the example directory:

```sh
npm test
npm run build
```

Focused Rust checks from the repository root are:

```sh
cargo check --locked -p vtcode-webmcp -p vtcode-core -p vtcode
cargo nextest run --locked -p vtcode-webmcp --no-fail-fast
cargo nextest run --locked -p vtcode -E 'test(/webmcp/)' --no-fail-fast
```

### Manual evidence still to capture

- TODO: Record the actual Chrome or ChatGPT in-app browser tool-inspector pass,
  including selected tools, arguments, returned structures, and visible UI
  effects.
- TODO: Capture screenshots of the tool list, editor draft/diff, and changes
  panel.
- TODO: Record and upload the narrated YouTube demo, shorter than three
  minutes.

## Demo video outline (under three minutes)

1. **0:00–0:20:** Open the live editor and explain that it works without a
   bridge in safe fallback mode.
2. **0:20–0:50:** Show the WebMCP tool list and ask the agent to search for and
   open the greeting file.
3. **0:50–1:35:** Read the file, stage the exact digest-checked replacement,
   and show the generated diff.
4. **1:35–2:05:** Show that the browser tool surface has no approve/apply/revert
   operation; approve only through the visible review flow.
5. **2:05–2:40:** Briefly show the optional authenticated VT Code bridge and
   explain that terminal policy remains authoritative.
6. **2:40–2:55:** Show the deterministic eval command and summarize the
   human/agent handoff.

## Judging alignment

- **WebMCP Leverage:** Eight typed, discoverable tools; schemas, annotations,
  abort handling, bounded results, tool-change cleanup, and deterministic eval
  journeys.
- **Execution:** A deployed, usable editor with a no-credential fallback,
  browser review flow, optional authenticated bridge, tests, and setup guides.
- **Potential Impact:** A concrete path for agents to collaborate with people
  in code editors while preserving human review and terminal authority.
- **Creativity & Ambition:** A browser-native agent handoff that treats WebMCP
  as a collaboration surface, not a hidden automation shortcut.

## Known limitations

- WebMCP availability depends on the browser/client and its current feature
  flag, origin-trial enrollment, or rollout; the fallback editor remains usable
  without it. No origin-trial token is currently committed or configured for
  the public demo.
- The public page is a static fallback demo. A real local workspace and VT Code
  turn require the optional loopback bridge and an active VT Code session.
- The deterministic eval corpus verifies contracts and expected journeys; a
  real browser-agent evaluation is probabilistic and still needs to be recorded
  for submission evidence.
- Browser tools stage a draft but never claim direct filesystem authority.

## Devpost form fields to confirm

These are the live fields returned by the Devpost integration. Do not submit
until the participant replaces the TODO values with truthful answers.

| Field | Draft value |
| --- | --- |
| Submitter Type | TODO — Individual / Team of Individuals / Organization |
| Country of residence | TODO — choose the participant/team country |
| Organization name | N/A unless submitting for an organization |
| App Status | TODO — New / Existing |
| Existing-project explanation | If Existing: during this submission period, added the eight-tool WebMCP surface, bounded contracts, safe draft-edit handoff, eval corpus, documentation, and public deployment. Confirm this accurately describes the project history. |
| Live URL | <https://vinhnx.github.io/VTCode/> |
| Testing instructions | Use the judge instructions above; no credentials are required for fallback mode. The optional bridge is not required to evaluate browser WebMCP. |
| Public code repo | <https://github.com/vinhnx/VTCode> |
| WebMCP clients tested | Deterministic tests pass. TODO — confirm the real clients used (Chrome WebMCP flag, ChatGPT in-app browser, or both) after the manual pass. |
| AI tools leveraged | OpenAI Codex; WebMCP-capable browser client for the model-facing tool surface. |
| Learning level | TODO — None / Moderate / Significant |
| Career AI value | TODO — Yes / No |

## Readiness checklist

- [x] Working public URL responds and contains the WebMCP tool surface.
- [x] Public repository and open-source license are available.
- [x] WebMCP implementation, security boundary, deterministic evals, and
  documentation are in the pushed branch.
- [x] Focused JavaScript and Rust checks pass.
- [ ] Confirm final title; the assistant has not chosen it.
- [ ] Run and record a real Chrome or ChatGPT in-app browser-agent pass.
- [ ] Capture screenshots.
- [ ] Upload the public narrated YouTube video under three minutes.
- [ ] Confirm personal/team form fields and whether the app is New or Existing.
- [ ] Run the final Devpost preflight, then explicitly authorize any external
  project update or submission.
