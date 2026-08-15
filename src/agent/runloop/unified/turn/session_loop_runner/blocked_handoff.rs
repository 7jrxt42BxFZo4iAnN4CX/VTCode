use std::path::Path;

use vtcode_core::core::agent::blocked_handoff::{BlockedHandoffResume, write_blocked_handoff_with_resume};
use vtcode_core::core::agent::harness_artifacts::existing_harness_artifact_paths;
use vtcode_core::exec::events::HarnessEventKind;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::{
    SessionArchive, SessionProgressArgs, SessionProgressPersistenceStatus, VerifiedSessionArchiveIdentifier,
};

use crate::agent::runloop::unified::inline_events::harness::{HarnessEventEmitter, harness_event};

const NO_ARCHIVE_RESUME_EXPLANATION: &str = "Resume is unavailable because no session archive exists.";
const UNVERIFIED_RESUME_EXPLANATION: &str = "Resume is unavailable because the session archive could not be verified.";

#[derive(Debug)]
enum ResumeAvailability {
    Available(VerifiedSessionArchiveIdentifier),
    Unavailable(String),
}

impl ResumeAvailability {
    fn as_handoff_resume(&self) -> BlockedHandoffResume<'_> {
        match self {
            Self::Available(identifier) => BlockedHandoffResume::Available(identifier),
            Self::Unavailable(explanation) => BlockedHandoffResume::Unavailable(explanation),
        }
    }
}

pub(super) struct SessionCheckpointOutcome {
    history_checkpoint_succeeded: bool,
    history_persistence_disabled: bool,
    blocked_resume: Option<ResumeAvailability>,
}

impl SessionCheckpointOutcome {
    fn new(blocked_turn: bool) -> Self {
        Self {
            history_checkpoint_succeeded: false,
            history_persistence_disabled: false,
            blocked_resume: blocked_turn.then(|| ResumeAvailability::Unavailable(UNVERIFIED_RESUME_EXPLANATION.into())),
        }
    }

    pub(super) fn without_archive(blocked_turn: bool) -> Self {
        let mut outcome = Self::new(blocked_turn);
        if blocked_turn {
            outcome.blocked_resume = Some(ResumeAvailability::Unavailable(NO_ARCHIVE_RESUME_EXPLANATION.into()));
        }
        outcome
    }

    pub(super) fn history_checkpoint_succeeded(&self) -> bool {
        self.history_checkpoint_succeeded
    }

    pub(super) fn history_persistence_disabled(&self) -> bool {
        self.history_persistence_disabled
    }

    pub(super) fn blocked_handoff_resume(&self) -> BlockedHandoffResume<'_> {
        self.blocked_resume.as_ref().map_or(
            BlockedHandoffResume::Unavailable(UNVERIFIED_RESUME_EXPLANATION),
            ResumeAvailability::as_handoff_resume,
        )
    }
}

pub(super) async fn persist_session_checkpoint(
    archive: &SessionArchive,
    args: SessionProgressArgs,
    blocked_turn: bool,
) -> SessionCheckpointOutcome {
    let mut outcome = SessionCheckpointOutcome::new(blocked_turn);
    let checkpoint_status = if blocked_turn {
        archive.persist_progress_async_with_status_forced(args).await
    } else {
        archive.persist_progress_async_with_status(args).await
    };

    match checkpoint_status {
        Ok(SessionProgressPersistenceStatus::Persisted(path)) => {
            outcome.history_checkpoint_succeeded = true;
            if blocked_turn {
                outcome.blocked_resume = Some(match archive.verify_persisted_resume_identifier(&path).await {
                    Ok(Some(identifier)) => ResumeAvailability::Available(identifier),
                    Ok(None) => ResumeAvailability::Unavailable(UNVERIFIED_RESUME_EXPLANATION.into()),
                    Err(err) => {
                        tracing::warn!(error = %err, "Failed to verify persisted session archive for blocked handoff");
                        ResumeAvailability::Unavailable(format!(
                            "Resume is unavailable because the persisted session archive could not be resolved: {err}"
                        ))
                    }
                });
            }
        }
        Ok(SessionProgressPersistenceStatus::Throttled(path)) => {
            tracing::debug!(
                path = %path.display(),
                "Session progress checkpoint throttled; retaining in-flight steering intents"
            );
            if blocked_turn {
                outcome.blocked_resume = Some(ResumeAvailability::Unavailable(
                    "Resume is unavailable because the blocked-turn checkpoint was throttled.".to_owned(),
                ));
            }
        }
        Ok(SessionProgressPersistenceStatus::Disabled(path)) => {
            outcome.history_persistence_disabled = true;
            tracing::debug!(
                path = %path.display(),
                "Session progress checkpoint skipped because history persistence is disabled"
            );
            if blocked_turn {
                outcome.blocked_resume = Some(ResumeAvailability::Unavailable(
                    "Resume is unavailable because history persistence is disabled.".to_owned(),
                ));
            }
        }
        Err(err) => {
            tracing::warn!(error = %err, "Failed to persist session progress");
            if blocked_turn {
                outcome.blocked_resume = Some(ResumeAvailability::Unavailable(format!(
                    "Resume is unavailable because the blocked-turn checkpoint failed: {err}"
                )));
            }
        }
    }

    outcome
}

pub(super) fn write_blocked_handoff_after_checkpoint(
    workspace: &Path,
    session_id: &str,
    blocker_summary: &str,
    resume: BlockedHandoffResume<'_>,
    renderer: &mut AnsiRenderer,
    harness_emitter: Option<&HarnessEventEmitter>,
) {
    match write_blocked_handoff_with_resume(
        workspace,
        session_id,
        "blocked",
        blocker_summary,
        &existing_harness_artifact_paths(workspace),
        resume,
    ) {
        Ok(artifacts) => {
            for path in [&artifacts.current_path, &artifacts.archive_path] {
                let path_text = path.display().to_string();
                let _ = renderer.line(MessageStyle::Info, &format!("Blocked handoff: {path_text}"));
                if let Some(emitter) = harness_emitter {
                    let _ = emitter.emit(harness_event(
                        HarnessEventKind::BlockedHandoffWritten,
                        Some("Blocked handoff written".to_string()),
                        Some(path_text),
                        None,
                        None,
                    ));
                }
            }
        }
        Err(err) => tracing::warn!(error = %err, "Failed to persist blocked handoff"),
    }
}
