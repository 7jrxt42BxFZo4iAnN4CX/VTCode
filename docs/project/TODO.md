~~https://github.com/EmbarkStudios/crash-handling~~
✅ Evaluated 2026-08-06: NOT adopted — existing panic handling (human_panic +
better_panic + color-eyre) is sufficient for a CLI/TUI tool. The crash-handling
crate's IPC-based minidump pipeline is designed for server architectures, not
CLI tools. VT Code's unsafe surface is <10 blocks, all FFI, all miri-audited.
See decisions.md 2026-08-06 for full rationale and revisit criteria.

~~https://github.com/EmbarkStudios/cargo-about~~
✅ Adopted 2026-08-06: Automated THIRD-PARTY-NOTICES generation. Added
about.toml (accepted licenses), scripts/templates/third-party-notices.hbs
(Handlebars template), scripts/generate-notices.sh (generation + --check for
CI), and license-notices CI job in ci.yml.

---

~~https://ohadravid.github.io/posts/2026-08-unsafe-water/~~
✅ Applied 2026-08-06: "Non-buoyant water" SAFETY comment audit — fixed broken
SAFETY comment in pipe.rs (IO-critical spawn path), restructured native_plugin.rs
module doc. See gotchas.md 2026-08-06.

---

integrate NVIDIA API

https://build.nvidia.com/explore/discover

---

https://www.greyblake.com/blog/branchless-rust/

--

lession learn, check feedback https://news.ycombinator.com/item?id=49176038

✅ Applied 2026-08-06: Audited VT Code against the Pi-minimalism HN feedback.
Findings:

- Compaction keeps last ~20k tokens: ALREADY implemented
  (`DEFAULT_RETAINED_USER_MESSAGE_TOKENS = 20_000` in compaction/mod.rs).
- Turn-boundary compaction: ALREADY implemented (`force_compaction` is set only
  by the runloop boundary, never by a tool loop — compaction/auto.rs).
- Post-compaction steer re-injection: ALREADY implemented (steering intents are
  snapshotted into the session memory envelope at compaction time).
- Long-running command polling burns tokens (codex $20/h "yep still running"):
  FIXED. The default run path yielded every 10s with a poll-oriented
  `next_continue_args`, pushing the model into a token-burning poll loop. Added
  `next_wait_args` (pre-filled `write_stdin action:"wait"`, 600s deadline) +
  `next_action_hint` to every still-running exec response, steering the model to
  the no-burn `wait` action (blocks in-harness up to the
  `long_running_command_ceiling_seconds`, no model round-trips while waiting).
  See exec_support.rs `attach_long_command_wait_steering` + guidelines.rs.
- Smaller system prompt / fewer tools (Pi's advantage): NOT actionable as a
  surgical change — architectural; VT Code's progressive tool documentation mode
  already defers tool descriptions to limit prompt size.

===

the compaction does not compact whole context, but keeps last ~20k tokens as is, I believe this helps a lot to model to not get confused what it is doing right now.

it also have soft/soft compaction limit, it tries to compact on turn boundary when possible. with combining with above this can get you about 35% more context (at least it looks like this with the sol)

codex when shell command is executed, will pull output with hard cap at max 30s, so for running compilation it will burn tokens without any benefit.

I have some tasks where agent will have to run some suite that can take over an hour, and codex burns about $20/h just waiting and reasoning every 30s "yep, that's still running". And what is going to happen after compaction, when whole context was just waiting? it will loose the plot and when I'm back it just does completely different thing that I asked it to do.

codex also have a bug, that opening refuses to resolve that adds your last steer after compaction, so imagine that you asked it to cleanup some tmp files or refactor/simplify something. it will do that again and again after each compaction, best case it just burns tokens and figures out, this is already done, or worse do it again and mess up everything and forget about it's task
