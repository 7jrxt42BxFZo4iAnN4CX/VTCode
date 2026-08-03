# Runtime Guidance and Project Instructions

VT Code has two distinct prompt sources:

| Source | Loaded from | Purpose | Trust boundary |
| --- | --- | --- | --- |
| Compiled runtime guidance | `crates/codegen/vtcode-core/src/prompts/runtime_guidance.rs` | Small, universal user-facing behavior included in Default, Minimal, Lightweight, and Specialized profiles | Part of the application runtime |
| Project instruction map | User/workspace `AGENTS.md`, `CLAUDE.md`, and `.vtcode/rules/` | Project conventions, local architecture, and maintainer workflows | User-controlled context, never a security boundary |

The compiled section is deterministic, cached with the static profile, and
kept below its approximate 256-token cap. It must not read, embed, or generate
content from repository instruction files. Profile-specific operating details
remain in the prompt builder; correctness-critical behavior belongs in runtime
policy, schemas, tests, or lints.

Even when `.vtcode/prompts/system.md` replaces the static base prompt, the
compiled section is reattached after prompt layers are resolved. This keeps the
universal baseline present without treating workspace prompt content as a
security boundary.

The dynamic instruction pipeline remains enabled by default. It discovers user
and workspace sources in precedence order, loads nested files for the active
directory, applies path-scoped rules and exclusions, and appends the resulting
project appendix separately from the compiled base prompt. `AGENTS.md` files
therefore remain useful maintainer maps without becoming an implicit source of
universal VT Code behavior.

When changing this boundary, run:

```bash
cargo nextest run -p vtcode-core
cargo check --locked
./scripts/check-dev.sh --changed
```

Release archives are independently allowlisted to contain the binary, man
page, and shell completions only. They must never include `AGENTS.md` or other
workspace guidance.
