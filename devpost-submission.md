# VT — The WebMCP Challenge submission draft

### ⏳ Not submitted yet

Nothing has been sent to Devpost.

This draft is prepared from the live Devpost requirements fetched on 2026-08-29.
The project is still a Devpost draft. The public narrated video, logged-out
playback, caption synchronization, and personal/team form answers are now
participant-confirmed; the final Devpost submit confirmation is still required.

## One-line Summary

VT is a browser coding workspace where WebMCP agents inspect code, stage one
digest-checked draft edit, and hand it to VT Code for human-reviewed approval.

## Problem

Browser agents often have to infer an editor's state from pixels, DOM labels,
and keyboard shortcuts. That makes simple coding tasks fragile, while giving an
agent broad filesystem write access makes the workflow difficult to trust.

## Solution

VT exposes a focused, structured workflow for people and agents to work together.
The browser can list files, search code, read a file and its digest, open the
file, stage one exact replacement, review the resulting diff, and navigate the
editor. The person reviews the proposed change. The browser tool surface cannot
approve, apply, or revert a filesystem change; an optional authenticated VT Code
bridge keeps terminal policy authoritative for real workspaces and agent turns.

The public page also has a deterministic in-memory fallback. Judges can use the
editor without credentials or a local backend, while the same page reports when
native WebMCP is unavailable and keeps the fallback usable.

## Why This Matters

WebMCP makes the editor's meaningful operations discoverable and composable for
agents instead of forcing them to guess at UI interactions. An agent can use a
bounded, typed result—such as a search match or file digest—as the next tool
input, while the person sees the draft and retains approval authority. This
turns browser-agent coding from opaque automation into a legible collaboration
loop.

## How We Used AI

The browser's native WebMCP surface is the model-facing interface. A compatible
browser agent can discover and invoke the eight registered tools using their
titles, JSON Schemas, annotations, abort signals, bounded outputs, and recovery
errors. The application does not bundle a model runner or claim that every
browser supports WebMCP; the real browser-agent pass must be run in ChatGPT's
in-app browser or Chrome with WebMCP enabled.

## How We Used Codex

OpenAI Codex was used to inspect the existing VT Code bridge, implement and
harden the browser tool contracts and Rust workspace adapter, add deterministic
browser evals, migrate the WebMCP app to strict TypeScript and Bun, write
the documentation, and run focused Rust and frontend verification. The final
project description is edited to match the implementation rather than being
treated as an unreviewed AI-generated claim.

## Key Features

- Eight bounded WebMCP tools for file listing, search, reading, editor state,
  opening files, exact draft edits, diff review, and panel navigation.
- CodeMirror 6 editor with tabs, syntax highlighting, line numbers, dirty draft
  state, and a unified diff review panel.
- Deterministic fallback workspace that keeps edits, proposals, checks, and
  reverts in page memory only.
- Authenticated loopback-oriented VT Code WebSocket bridge with one-time pairing,
  exact origin validation, bounded frames, event replay, and reconnect support.
- Digest-checked proposals and fail-closed handling for stale files, traversal,
  symlink/hard-link escapes, sensitive paths, and unsafe checks.
- Explicit separation between browser draft mutation and terminal filesystem
  authority.
- Workflow-state hints, truncation metadata, tool-change cleanup, cancellation,
  and actionable recovery errors.

## Architecture

The Vite WebMCP app in `apps/webmcp/` owns the editor state,
fallback backend, native WebMCP registration, and authenticated bridge client.
The Rust `vtcode-webmcp` crate is the first-class bridge shipped in VT Code; it
owns pairing, origin checks, token authentication, bounded workspace operations,
digest validation, event replay, and runtime adapter boundaries. The main
`vtcode` binary exposes `/webmcp` TUI commands and `vtcode webmcp` CLI commands.
The connected browser sends structured requests; VT Code remains responsible
for policy, approval, and authoritative file changes.

## Testing Instructions

### Public browser path

1. Open <https://vinhnx.github.io/VTCode/>.
2. Close **Settings** if it opens automatically; fallback mode needs no
   credentials.
3. In ChatGPT's in-app browser, or in Chrome 150.0.7861.0+ with
   `chrome://flags/#enable-webmcp-testing` enabled and the WebMCP Model Context
   Tool Inspector installed, inspect the registered tools.
4. Ask the agent to list files, search for the greeting, open the result, read
   it, stage one exact replacement using its digest, review the diff, and open
   the changes panel. A real ChatGPT in-app browser run has completed this
   sequence locally with eight successful tool calls.
5. Confirm long results report truncation and that no exposed browser tool can
   approve, apply, or revert a filesystem change.
6. Before the run, open **Evidence**, select the actual client, and choose
   **Start new run**. Afterward export the sanitized JSON with **Copy JSON** or
   **Download JSON**, and keep it beside the client tool-inspector screenshot
   or screen recording. The export records real page callbacks, not a simulated
   self-check; it omits file contents, diffs, prompts, pairing codes, and tokens.

If native WebMCP is unavailable, the editor's fallback remains usable for the
same human review flow, but the WebMCP requirement should be verified in an
eligible browser before judging.

### Local WebMCP app checks

From `apps/webmcp/`:

```sh
bun install --frozen-lockfile
bun run typecheck
bun run test
bun run build
```

### Rust bridge checks

From the repository root:

```sh
cargo check --locked -p vtcode-webmcp -p vtcode-core -p vtcode
cargo nextest run --locked -p vtcode-webmcp --no-fail-fast
cargo nextest run --locked -p vtcode -E 'test(/webmcp/)' --no-fail-fast
```

## Public Demo Link

<https://vinhnx.github.io/VTCode/>

The live page is a static WebMCP app. It does not expose a local
filesystem and does not require credentials in fallback mode.

## Public Repository Link

<https://github.com/vinhnx/VTCode>

The repository is public and includes the Apache-2.0 license in `LICENSE`.

## Demo Video

A final render with the recorded audio is ready locally at
`/Users/vinhnguyenxuan/Documents/Codex/2026-08-29/webmcp-evidence/outputs/vt-webmcp-production-subtitles-recorded-audio-restart.mp4`.
It is 80 seconds, shows the sanitized Ghostty bridge, the public connected
client, Chrome evidence, recorded ChatGPT client evidence, and the review /
approval / verification flow. The recorded M4A is the audio track; play it once
to confirm narration clarity and synchronization before publishing.

The participant provided this public YouTube URL for the video:
<https://www.youtube.com/watch?v=feU7ARLPxa0>. The participant confirmed
logged-out playback and caption synchronization.

The copy-paste YouTube title, description, chapters, captions path, and aligned
80-second narration script are in
[`webmcp-youtube-upload-package.md`](webmcp-youtube-upload-package.md). Keep
the subtitles; the combined render now contains the recorded explanatory audio.

## Screenshot Shot List

Capture 3–5 screenshots before submitting:

1. The public connected editor with the workspace tree and open `README.md`.
2. The public connected Settings panel showing origin, lease, limits, and policy.
3. The Chrome inspector evidence panel showing the public origin and eight tools.
4. The recorded ChatGPT in-app-browser run with one discovery event and eight calls.
5. The staged exact edit and unified diff in the **CHANGES** panel.

## Submission Readiness Notes

The live Devpost requirements require a working hosted URL, a text description
covering WebMCP fit, user experience, people/agent collaboration, and
implementation, a public open-source repository, and a public demo video under
three minutes with audio. No website field or zip file is required. The live
URL and public repository are present, and the repository contains source,
instructions, and a visible open-source license.

The live event is accepting submissions. The official rules list the submission
deadline as September 3, 2026 at 1:00 PM Pacific; after submissions close, the
submission, repository, deployment, and video should remain frozen until winners
are announced.

The repository history documents meaningful WebMCP work after the submission
period began on August 25, 2026. The participant confirmed that **Existing**
accurately describes the project and keeps the prior/new work distinction clear.

Current evidence is strong for deterministic contracts, security boundaries,
fallback behavior, and Rust/TypeScript tests. The fresh public Chrome inspector
JSON (`vt-webmcp-real-chrome-inspector-evidence-restart.json`) and screenshot
(`vt-webmcp-public-chrome-evidence-restart.png`) record one public-origin
discovery event and zero inspector callbacks. The separate ChatGPT in-app
browser JSON (`vt-webmcp-real-client-evidence.json`) and screenshot
(`vt-webmcp-real-client-evidence.png`) record one discovery event and eight
successful tool calls from an earlier local client run; these are intentionally
described as separate client sessions.
The public deployment now serves the tested build from the repository Pages path;
the public video and personal/team eligibility answers are participant-confirmed.

A local preflight scan found no high-confidence secret patterns in tracked
source. An ignored, untracked root `.env` exists and must remain private; review
it before final submission. No `.pem`, `id_rsa`, or `id_dsa` files were found.

## Known Limitations

- Native WebMCP availability depends on the browser/client and its current
  rollout, origin-trial enrollment, or testing flag.
- The public page is a static WebMCP app. Real workspace access and VT Code
  turns require the optional local bridge and an active VT Code session.
- Deterministic evals verify contracts and journeys; browser-agent tool selection
  remains probabilistic and must be demonstrated manually.
- Browser tools stage drafts but never claim direct filesystem authority.

## Official Form Fields

These values come from the live Devpost form. The participant confirmed and saved
the required values in Devpost; the final submit remains gated by explicit
confirmation.

| Field                        | Draft value                                                                                                                                                                                                                       |
| ---------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Submitter Type               | Individual                                                                                                                                                                                                                        |
| Country of residence         | Vietnam                                                                                                                                                                                                                           |
| Organization name            | N/A unless submitting for an organization                                                                                                                                                                                         |
| App Status                   | Existing                                                                                                                                                                                                                          |
| Existing-project explanation | If Existing: during the submission period, added the WebMCP browser tool surface, bounded contracts, safe draft-edit handoff, eval corpus, bridge integration, documentation, and public deployment. Confirm accuracy before use. |
| Live URL                     | <https://vinhnx.github.io/VTCode/>                                                                                                                                                                                                |
| Testing instructions         | Use the instructions above; start an Evidence run before the external client, export the sanitized JSON, and include the client screenshot/video. No credentials are required for fallback mode.                                  |
| Public code repo             | <https://github.com/vinhnx/VTCode>                                                                                                                                                                                                |
| WebMCP clients tested        | Chrome WebMCP Tool Inspector — public origin, 1 discovery event and 0 callbacks in the fresh run; ChatGPT in-app browser — 1 discovery event and 8/8 successful real tool calls in the earlier local run                          |
| AI tools leveraged           | OpenAI Codex; the WebMCP-capable browser agent used for the manual pass                                                                                                                                                           |
| Learning level               | Significant                                                                                                                                                                                                                       |
| Career AI value              | Yes                                                                                                                                                                                                                               |
| Demo video URL               | <https://www.youtube.com/watch?v=feU7ARLPxa0> — participant confirmed logged-out playback and caption synchronization                                                       |

## Final Checklist

- [x] Working public URL responds.
- [x] Public repository is accessible and contains an open-source license.
- [x] Text description explains WebMCP fit, the collaboration loop, and the
      implementation.
- [x] Focused WebMCP TypeScript/Bun and Rust checks pass locally.
- [x] Run and record a real ChatGPT in-app browser-agent pass; save the sanitized Evidence JSON.
- [x] Pair the public GitHub Pages client to a fresh Ghostty WebMCP bridge.
- [x] Capture public connected, Settings, and Chrome inspector evidence screenshots.
- [x] Download the fresh public-origin sanitized Chrome inspector JSON.
- [x] Verify the provided YouTube URL is public, playable while logged out, under three minutes, and has clear explanatory audio (participant-confirmed).
- [x] Confirm caption synchronization (participant-confirmed).
- [x] Confirm and save personal/team form fields, residence, app status, learning level,
      and career value in Devpost Additional info (Individual; Vietnam; Existing;
      Significant; Yes; saved with participant authorization).
- [x] Run the final Devpost preflight; explicit submission authorization remains pending.
