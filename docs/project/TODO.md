check: https://quinn.inkboxwire.com/deep-audit-vtcode

The short version: the automated pass scored it 50, but the human-verified pass revises that to 78. The difference is that all five of the automated "secret" findings turned out to be false positives (OAuth client IDs, your sanitizer's own detection patterns, and test fixtures) — a good live example of why an automated score is a triage signal, not a verdict.

The genuinely useful bits are in the report: your sandbox/exec boundary is the crown jewel and the #1 thing to keep under continuous adversarial pressure, and the OAuth token-handling path is worth a focused review. Nothing alarming — the codebase reads as one where security was designed in.

If any of it's useful, feel free to share the link anywhere. And if you spot something in the report you think is wrong, tell me — I'd rather be corrected than believed. the sandbox-boundary recommendation is the one I'd keep top-of-mind as you add new tools/surfaces.

---

check /update and self update inside the vtcode TUI program seems hangs and doesn't have real time feedback when downloading and extracting the update. It would be nice to have a progress bar or some kind of feedback to indicate that the update is in progress and not just hanging. check how CLI implementations handle this and see if we can improve the user experience in the TUI.
