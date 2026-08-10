/// Granular progress events emitted during the self-update pipeline.
///
/// The TUI install path consumes these to give real-time feedback while the
/// download, checksum verification, extraction, and binary replacement run,
/// so the UI does not appear to hang on a single "Updating..." message.
#[derive(Clone, Debug)]
pub(crate) enum UpdateProgress {
    /// Downloading the release archive.
    ///
    /// `downloaded` and `total` are bytes; `total` is `None` when the server
    /// omits `Content-Length`. Emitted with throttling so callers can forward
    /// each event directly to the UI without flooding the render channel.
    Downloading { downloaded: u64, total: Option<u64> },
    /// Verifying the archive checksum (sidecar download + hashing).
    VerifyingChecksum,
    /// Extracting the archive and staging the new binary.
    Extracting,
    /// Replacing the current binary via `self_replace`.
    ReplacingBinary,
}
