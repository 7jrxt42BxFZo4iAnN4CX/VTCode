# TODO

## Memory fixes

- [ ] Investigate and fix the memory feature not saving user context.
    - **Observed:** After asking VT Code to remember that the user is Vinh Nguyen / `vinhnx`, the assistant reported: “Couldn't save memory because the LLM planner still needs more information.”
    - **Expected:** A sufficiently specific, user-approved fact should be persisted to the session-independent memory store and be available in later turns.
    - **Reproduction context:** The request followed a conversation in which `vinhnx` was identified from local repository metadata and public profiles. The save attempt failed before any confirmation that a memory file or durable store entry was created.
    - **Acceptance criteria:** - Saving a clear user preference or identity alias does not require unrelated planner information. - The user receives an actionable error when persistence fails, including what additional information is required. - A successful save is verified by reading the memory through the supported memory path in a subsequent turn. - Add regression coverage for the planner/memory-save path and the failure message above.
      log: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/checkpoints/turn_995.json /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/checkpoints/turn_994.json /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/checkpoints/turn_993.json /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/checkpoints/turn_992.json /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/checkpoints/turn_991.json

session: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/sessions/session-vtcode-20260825T035810Z_038177-75164

---

CRITICAL: check vtcode post-amble summaried session is not gone/missing. it was working before. context: when user control+c or quit the program, there is the summarization turn/context shown in the CLI. Currently it showing a blank space. This is a regression from the previous behavior. The summarization turn/context should be shown in the CLI after the user quits the program.
