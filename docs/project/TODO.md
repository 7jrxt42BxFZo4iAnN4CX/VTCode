===

https://github.com/vinhnx/VTCode/security/advisories/GHSA-r249-hpfx-x2w7

===

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

You can also suggest to improve it.
