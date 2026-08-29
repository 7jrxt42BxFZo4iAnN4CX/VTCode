# VT YouTube Upload Package

This package is prepared for the WebMCP Challenge demo video. The final upload
render is 80 seconds, 1280x720, H.264 video with AAC audio:

`/Users/vinhnguyenxuan/Documents/Codex/2026-08-29/webmcp-evidence/outputs/vt-webmcp-production-subtitles-recorded-audio-restart.mp4`

## Upload settings

- Title: `VT — A Human-Approved WebMCP Coding Workspace`
- Visibility: `Public`
- Audience: Not made for kids
- Language: English
- Captions: Upload `vt-webmcp-production-subtitles-restart.srt`
- Length: 1:20, within the required three-minute limit
- YouTube URL: https://www.youtube.com/watch?v=feU7ARLPxa0

The recorded M4A has been combined with the subtitle video as the final render.
The recording is 79.04 seconds; the last 0.96 seconds are padded with silence so
the 80-second visual ending remains intact. The participant confirmed that the
public YouTube video plays while logged out and that the subtitles are
synchronized.
Subtitles alone do not satisfy the live requirement that the video include audio
covering what was built and how WebMCP was used.

## YouTube description

VT is a browser coding workspace where people and agents can inspect code,
stage one digest-checked draft edit, and hand it to VT Code for human-reviewed
approval.

This demo shows:

- a real VT Code Ghostty bridge serving a bounded workspace;
- the public GitHub Pages client and its explicit trust boundary;
- eight discoverable WebMCP tools in Chrome's WebMCP inspector;
- sanitized evidence from the public Chrome discovery run;
- a separate earlier ChatGPT in-app-browser run with eight successful tool calls;
- exact digest-checked staging, unified diff review, approval gating, verification,
  and restore.

WebMCP gives an agent typed, bounded editor operations instead of requiring it to
guess from pixels or keyboard shortcuts. The browser can stage a draft, but it
cannot approve, apply, or revert a filesystem change. VT Code remains authoritative
for terminal policy and real workspace mutations.

Live demo: https://vinhnx.github.io/VTCode/
Source code: https://github.com/vinhnx/VTCode
Challenge: https://webmcp.devpost.com/
Demo video: https://www.youtube.com/watch?v=feU7ARLPxa0

Chapters:

00:00 Real Ghostty bridge
00:08 Live production client
00:16 Trust boundary
00:24 Chrome inspector run
00:32 ChatGPT client evidence
00:40 Deterministic checks
00:48 Review before write
00:56 Approval gate
01:04 Bounded apply
01:12 Verify and restore

## Short timed script

This replaces the longer script. Each line is short enough for its eight-second
scene and can be used both as subtitles and as a spoken guide. Keep the
subtitles visible so the demo remains understandable with sound muted.

### 00:00–00:08

VT is a browser workspace for people and agents to edit code together.

### 00:08–00:16

VT Code serves a bounded workspace; the pairing credential is redacted.

### 00:16–00:24

The app shows origin, lease, limits, and VT Code policy.

### 00:24–00:32

Chrome discovers eight WebMCP tools on the public origin.

### 00:32–00:40

An earlier ChatGPT run recorded eight successful tool calls.

### 00:40–00:48

The browser workflow stays deterministic and inspectable.

### 00:48–00:56

The exact unified diff stays visible before approval.

### 00:56–01:04

The proposal is ready, but the workspace is unchanged.

### 01:04–01:12

VT Code controls writes; browser memory stays isolated.

### 01:12–01:20

Verification passes, and the original state can be restored.

## Final upload check

- [x] Combine the recorded explanatory audio with the MP4.
- [ ] Play the combined file and confirm the narration is clear and synchronized.
- [x] Confirm the final video is public, playable while logged out, and under three minutes.
- [x] Confirm subtitle synchronization (participant-confirmed).
- [ ] Upload the SRT captions.
- [x] Copy the provided YouTube URL into `devpost-submission.md`.
- [ ] Keep the public demo, source repository, and video claims consistent.
