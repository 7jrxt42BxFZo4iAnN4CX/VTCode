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

--

lession learn, check feedback https://news.ycombinator.com/item?id=49176038

===

the compaction does not compact whole context, but keeps last ~20k tokens as is, I believe this helps a lot to model to not get confused what it is doing right now.

it also have soft/soft compaction limit, it tries to compact on turn boundary when possible. with combining with above this can get you about 35% more context (at least it looks like this with the sol)

codex when shell command is executed, will pull output with hard cap at max 30s, so for running compilation it will burn tokens without any benefit.

I have some tasks where agent will have to run some suite that can take over an hour, and codex burns about $20/h just waiting and reasoning every 30s "yep, that's still running". And what is going to happen after compaction, when whole context was just waiting? it will loose the plot and when I'm back it just does completely different thing that I asked it to do.

codex also have a bug, that opening refuses to resolve that adds your last steer after compaction, so imagine that you asked it to cleanup some tmp files or refactor/simplify something. it will do that again and again after each compaction, best case it just burns tokens and figures out, this is already done, or worse do it again and mess up everything and forget about it's task
