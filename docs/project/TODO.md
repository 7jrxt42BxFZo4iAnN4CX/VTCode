Option C: Replace self_update with self-replace crate (no quick-xml dependency, but requires code changes)

===

https://github.com/vinhnx/VTCode/security/advisories/GHSA-r249-hpfx-x2w7

===

check update checker wrong. the installed version is latest but the popup on launch still repeatly ask to update

Last login: Tue Jul 28 13:44:58 on ttys007

> VT Code (0.140.2) Build · StepFun Step-3.7-Flash (262K) · medium
> ---------------------------------- Info -----------------------------------

    Update available! 0.140.2 -> 0.141.2
    Run /update install, or `vtcode update` from the CLI, to update.
    See full release notes:
    https://github.com/vinhnx/vtcode/releases/tag/0.141.2

---

Update available
─────────────────────────────────────────────────────────────────────────────
VT Code 0.140.2 -> 0.141.2
• Release notes: https://github.com/vinhnx/vtcode/releases/tag/0.141.2
Navigation: ↑/↓ select • Space/Enter apply • ←/→ change value • Esc close
│ [Recommended] Update and restart
Run the documented install command and relaunch VT Code.

Stay on current version
Dismiss for now. Run `vtcode update` when ready.

─────────────────────────────────────────────────────────────────────────────

===

check to increase tool limit, it was locked 32 steps

causing
////////////////////////////////// Error //////////////////////////////////
Safety validation failed: Per-turn tool limit reached (max: 32). Wait or
adjust config.
///////////////////////////////////////////////////////////////////////////

/Users/vinhnguyenxuan/.vtcode/sessions/debug-session-vtcode-20260728t005624z_773774-71727.log /Users/vinhnguyenxuan/.vtcode/sessions/session-vtcode-20260728T005624Z_773774-71727.json /Users/vinhnguyenxuan/.vtcode/sessions/atif-trajectory-session-vtcode-20260728T005624Z_773774-71727-20260728T005626Z.json /Users/vinhnguyenxuan/.vtcode/sessions/harness-session-vtcode-20260728T005624Z_773774-71727-20260728T005626Z.jsonl

===

command+click (macos) on a file url path in the terminal to open it in the editor, but it doesn't work. It opens a new tab instead of focusing on the existing tab. the file open in external editor being blocked until agent program end turn, which is not ideal. It should open the file in the editor immediately, and focus on the existing tab if the file is already open.
