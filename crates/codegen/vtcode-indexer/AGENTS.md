# vtcode-indexer

[Root AGENTS.md](../AGENTS.md) | Lightweight workspace file indexer with Markdown-backed persistence.

## Key Types

`SimpleIndexer` main API | `SimpleIndexerConfig` configuration | `FileIndex` entry | `SearchResult` match | `IndexStorage` trait | `TraversalFilter` trait | `MarkdownIndexStorage` default backend | `ConfigTraversalFilter` default filter

## Rules

- `IndexStorage` trait is the persistence extension point. Implement for custom backends.
- `TraversalFilter` trait controls which files/dirs are indexed. `ConfigTraversalFilter` is default.
- Always skips `.env`, `.git`, `.DS_Store` — hardcoded in `should_index_file()`.
- `SimpleIndexerConfig` excludes `target/`, `node_modules/`, `.vtcode/index/` by default.
- Snapshot storage (`prefers_snapshot_persistence()`) writes a single `index.md` — legacy per-file `.md` entries are auto-cleaned.

## Testing

`cargo nextest run -p vtcode-indexer`. Uses `tempfile::tempdir()` for isolation.

## Gotchas

- `FileIndexCache` builds traverse the workspace and use synchronous/Rayon work; keep them in `spawn_blocking` and preserve the single-build gate so Tokio workers remain responsive and concurrent misses do not stampede the filesystem.
- `index_directory()` respects `.gitignore` via `ignore` crate's `WalkBuilder`.
- `path_cache` is not used here — that's `vtcode-bash-runner`. Indexer uses `index_cache: HashMap`.
- Binary/unreadable files are silently skipped (`ErrorKind::InvalidData`).
