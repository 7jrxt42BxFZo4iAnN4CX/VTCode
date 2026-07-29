# Legacy Updater Compatibility Release

## Goal

Allow VT Code v0.141.0 through v0.141.4 to update automatically to v0.141.6
without requiring users to run the native installer manually, while preserving
the native archive updater introduced in v0.141.5.

## Root Cause

The affected releases use `self_update` 1.0.0-rc.6 with an asset identifier
ending in `<target>.tar.gz`. Releases v0.141.0 through v0.141.3 lack archive
support, while v0.141.4 enables `archive-tar` but not
`compression-tar-gz`. None can install a gzip-compressed tar archive.

The old asset matcher uses substring matching. Its extractor determines the
format from the final filename extension and supports an uncompressed plain
binary independently of archive features.

## Release Asset Contract

For every supported target, v0.141.6 publishes:

1. A raw executable compatibility asset named
   `compat-vtcode-<version>-<target>.tar.gz.compat`.
2. The normal archive:
   - `<target>.tar.gz` for macOS and Linux.
   - `<target>.zip` for Windows.
3. SHA-256 metadata covering every executable asset.

> **The `compat-` prefix is load-bearing.** GitHub's releases API returns the
> `assets` array sorted alphabetically by name (ascending), and the legacy
> updater's `asset_for` returns the **first** asset whose name `contains(target)`
> and `contains("{target}.tar.gz")`. Both the normal `vtcode-<v>-<target>.tar.gz`
> and the compat asset match that substring test, so the alphabetically-first one
> wins. `compat-` sorts before `vtcode-` (`c` < `v`), so the compat asset is
> selected. Without the prefix (`vtcode-<v>-<target>.tar.gz.compat`) the normal
> archive sorts first and the legacy updater still hits
> `CompressionNotEnabledError`. Upload order is irrelevant because GitHub
> re-sorts by name.

Legacy updaters select the first matching asset (alphabetical order), then treat
the final `.compat` extension as a plain executable. The v0.141.5+ updater
requires `starts_with("vtcode-")` and `ends_with("{target}.tar.gz"` /
`"{target}.zip")` and therefore ignores compatibility assets.

Compatibility assets retain the platform executable name internally through
the legacy updater's `bin_name` setting: `vtcode` on Unix and `vtcode.exe` on
Windows.

## Pipeline Changes

- Generate compatibility assets from the exact binaries packaged into normal
  release archives.
- Include x86_64 Windows artifacts in the bridge **by default**
  (`RELEASE_REQUIRE_WINDOWS=false` to opt out) so every release rescues all
  users, including Windows v0.141.0-v0.141.4 whose updater used the
  `{target}.tar.gz` identifier that never matched the published `.zip`. Reserve
  the opt-out for emergency macOS/Linux rescues when Windows CI is flaky.
- Publish compatibility assets in a separate upload operation before normal
  assets (defense-in-depth only — GitHub re-sorts assets by name, so the
  `compat-` prefix, not upload order, is what drives legacy selection).
- Include compatibility assets in `checksums.txt`.
- Accept the repository's legacy sidecar convention
  (`archive.tar.gz` paired with `archive.sha256`) as well as
  `archive.tar.gz.sha256`.
- Fail release validation when a required target lacks its compatibility
  binary, normal archive, or checksum.

Windows ARM remains unsupported until the build workflow produces that target;
the updater must not claim release coverage that the pipeline cannot publish.

## Safety

- Compatibility assets are raw executables and must use bytes extracted from
  already-built release archives, not a separate build.
- Validate each asset digest before upload.
- Test legacy selection using the same substring and extension behavior as
  `self_update` 1.0.0-rc.6.
- Test current selection to prove compatibility assets are ignored.
- Do not mutate v0.141.5 assets.

## Verification

- Unit tests for compatibility naming, old/new selection, sidecar lookup, and
  required target coverage.
- Extract each normal archive and byte-compare its executable with the
  compatibility asset.
- Run updater tests through `cargo nextest`.
- Build v0.141.6 release binaries.
- Stage a draft release and inspect asset ordering through the GitHub API.
- Exercise a v0.141.4 updater fixture against the draft assets.
- Exercise the v0.141.6 updater against the normal archive.
- Publish only after both paths succeed.
