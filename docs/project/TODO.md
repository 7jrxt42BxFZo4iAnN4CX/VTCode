===

https://github.com/vinhnx/VTCode/security/advisories/GHSA-r249-hpfx-x2w7

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

====

check and group repeated/similar tool calls display in the TUI transition log, instead of showing each call separately. This will make the log more readable and easier to follow. Maybe introduce a compact tool display mode. Use current tool display as expanded mode, and add a compact mode that groups repeated/similar tool calls together. Allow users to toggle between the two modes in the TUI. Confirugable via /config on vtcode.toml. This will help users to focus on the important information and reduce noise in the log.

Example:

1. before

```
• Search code Use Code search
  └ Path: .
  └ File types: rs
  └ Result types: path
  └ Max results: 30
• Search code Use Code search
  └ Path: .
  └ File types: rs
  └ Result types: path
  └ Max results: 100
```

2. after

```
* Research
  └ Search max_tool_calls_per_turn|full_auto|max_conversation_turns `in` config.rs `with` result types: path ...
```
