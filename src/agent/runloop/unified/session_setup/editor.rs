use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;

use anyhow::{Context, Result};
use tokio::sync::mpsc::{self, Receiver, Sender};
use vtcode_commons::{EditorTarget, resolve_editor_target};
use vtcode_core::config::EditorToolConfig;
use vtcode_core::tools::terminal_app::TerminalAppLauncher;
use vtcode_ui::tui::app::InlineHandle;

use super::types::BackgroundTaskGuard;

const EDITOR_OPEN_REQUEST_CAPACITY: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EditorOpenRequest {
    target: EditorTarget,
}

impl EditorOpenRequest {
    pub(crate) fn new(target: EditorTarget) -> Self {
        Self { target }
    }

    pub(crate) fn from_raw_target(raw: &str, workspace: &std::path::Path) -> Option<Self> {
        resolve_editor_target(raw, workspace).map(Self::new)
    }
}

pub(crate) type EditorOpenRequestSender = Sender<EditorOpenRequest>;

pub(crate) fn bounded_editor_open_requests() -> (EditorOpenRequestSender, Receiver<EditorOpenRequest>) {
    mpsc::channel(EDITOR_OPEN_REQUEST_CAPACITY)
}

pub(crate) struct EditorOpenCoordinator {
    receiver: Receiver<EditorOpenRequest>,
    pending: VecDeque<EditorTarget>,
    queued_targets: HashSet<String>,
    active_targets: HashSet<String>,
}

impl EditorOpenCoordinator {
    pub(crate) fn new(receiver: Receiver<EditorOpenRequest>) -> Self {
        Self {
            receiver,
            pending: VecDeque::new(),
            queued_targets: HashSet::new(),
            active_targets: HashSet::new(),
        }
    }

    async fn next_pending_target(&mut self) -> Option<EditorTarget> {
        loop {
            if let Some(target) = self.pending.pop_front() {
                let key = target.canonical_string();
                self.queued_targets.remove(&key);
                self.active_targets.insert(key);
                return Some(target);
            }

            let request = self.receiver.recv().await?;
            self.enqueue(request.target);
        }
    }

    fn enqueue(&mut self, target: EditorTarget) {
        let key = target.canonical_string();
        if self.active_targets.contains(&key) || !self.queued_targets.insert(key) {
            return;
        }
        self.pending.push_back(target);
    }

    fn finish_target(&mut self, target: &EditorTarget) {
        // Drain duplicates that arrived while this launch was pending while
        // the target is still marked active. Requests arriving afterwards are
        // a new user action and may launch again.
        while let Ok(request) = self.receiver.try_recv() {
            self.enqueue(request.target);
        }
        self.active_targets.remove(&target.canonical_string());
    }

    async fn run(mut self, workspace: PathBuf, editor_config: EditorToolConfig, handle: InlineHandle) {
        while let Some(target) = self.next_pending_target().await {
            if !editor_config.enabled {
                tracing::debug!(target = %target.canonical_string(), "ignoring file-open request because external editors are disabled");
                self.finish_target(&target);
                continue;
            }

            let preferred_editor =
                (!editor_config.preferred_editor.trim().is_empty()).then(|| editor_config.preferred_editor.clone());
            let suspend_tui = editor_config.suspend_tui
                && TerminalAppLauncher::editor_command_requires_terminal(preferred_editor.as_deref());
            let launcher = TerminalAppLauncher::new(workspace.clone());
            let launch_result =
                launch_target(handle.clone(), suspend_tui, launcher, target.clone(), preferred_editor).await;
            if let Err(error) = launch_result {
                tracing::warn!(target = %target.canonical_string(), %error, "failed to open file from TUI");
            }
            self.finish_target(&target);
        }
    }
}

pub(crate) fn spawn_editor_open_coordinator(
    workspace: PathBuf,
    editor_config: EditorToolConfig,
    handle: &InlineHandle,
) -> (EditorOpenRequestSender, BackgroundTaskGuard) {
    let (sender, receiver) = bounded_editor_open_requests();
    let coordinator = EditorOpenCoordinator::new(receiver);
    let handle = handle.clone();
    let task = tokio::spawn(coordinator.run(workspace, editor_config, handle));
    (sender, BackgroundTaskGuard::new(task))
}

async fn launch_target(
    handle: InlineHandle,
    suspend_tui: bool,
    launcher: TerminalAppLauncher,
    target: EditorTarget,
    preferred_editor: Option<String>,
) -> Result<()> {
    crate::agent::runloop::unified::turn::session::slash_commands::run_with_event_loop_suspended_async(
        &handle,
        suspend_tui,
        || async move {
            tokio::task::spawn_blocking(move || launcher.launch_editor_target_non_waiting(target, preferred_editor))
                .await
                .map_err(|error| anyhow::anyhow!("editor launch task failed: {error}"))?
        },
    )
    .await
    .context("failed to launch editor")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use vtcode_commons::EditorTarget;

    #[tokio::test]
    async fn coalesces_duplicate_targets_while_a_launch_is_pending() {
        let (sender, receiver) = bounded_editor_open_requests();
        let mut coordinator = EditorOpenCoordinator::new(receiver);

        sender
            .send(EditorOpenRequest::new(EditorTarget::new(PathBuf::from("src/main.rs"), None)))
            .await
            .expect("send first request");
        sender
            .send(EditorOpenRequest::new(EditorTarget::new(PathBuf::from("src/main.rs"), None)))
            .await
            .expect("send duplicate request");
        drop(sender);

        assert_eq!(
            coordinator.next_pending_target().await,
            Some(EditorTarget::new(PathBuf::from("src/main.rs"), None))
        );
        coordinator.finish_target(&EditorTarget::new(PathBuf::from("src/main.rs"), None));
        assert_eq!(coordinator.next_pending_target().await, None);
    }

    #[tokio::test]
    async fn preserves_distinct_targets_in_bounded_request_order() {
        let (sender, receiver) = bounded_editor_open_requests();
        let mut coordinator = EditorOpenCoordinator::new(receiver);

        sender
            .send(EditorOpenRequest::new(EditorTarget::new(PathBuf::from("src/main.rs"), None)))
            .await
            .expect("send first request");
        sender
            .send(EditorOpenRequest::new(EditorTarget::new(PathBuf::from("src/lib.rs"), None)))
            .await
            .expect("send second request");
        drop(sender);

        let first_target = EditorTarget::new(PathBuf::from("src/main.rs"), None);
        let second_target = EditorTarget::new(PathBuf::from("src/lib.rs"), None);
        assert_eq!(coordinator.next_pending_target().await, Some(first_target.clone()));
        coordinator.finish_target(&first_target);
        assert_eq!(coordinator.next_pending_target().await, Some(second_target.clone()));
        coordinator.finish_target(&second_target);
        assert_eq!(coordinator.next_pending_target().await, None);
    }
}
