https://github.com/vinhnx/VTCode/actions/runs/31323157822

https://github.com/vinhnx/VTCode/actions/workflows/secret-scan.yml

https://github.com/vinhnx/VTCode/issues/723#issuecomment-5232024558

===

A little thank-you for being an early supporter — I went ahead and ran the full deep audit (the paid tier) on VTCode, free, and wanted you to have it first:

quinn.inkboxwire.com/deep-audit-vtcode

The short version: the automated pass scored it 50, but the human-verified pass revises that to 78. The difference is that all five of the automated "secret" findings turned out to be false positives (OAuth client IDs, your sanitizer's own detection patterns, and test fixtures) — a good live example of why an automated score is a triage signal, not a verdict.

The genuinely useful bits are in the report: your sandbox/exec boundary is the crown jewel and the #1 thing to keep under continuous adversarial pressure, and the OAuth token-handling path is worth a focused review. Nothing alarming — the codebase reads as one where security was designed in.

If any of it's useful, feel free to share the link anywhere. And if you spot something in the report you think is wrong, tell me — I'd rather be corrected than believed.

check: https://quinn.inkboxwire.com/deep-audit-vtcode

--

https://github.com/vinhnx/VTCode/actions/runs/31355776083/job/93355058550
1s
Scan first-party debt markers
0s
1
Run ./scripts/first-party-debt-scan.sh
6
first-party debt scan requires ripgrep (rg)
7
Error: Process completed with exit code 2.
