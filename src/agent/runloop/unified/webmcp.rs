use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use uuid::Uuid;
use vtcode_config::{WebmcpConfig, loader::VTCodeConfig};
use vtcode_webmcp::runtime::{
    AppliedChange, CheckResult, FileSnapshot, PatchProposal, RuntimeAdapter, RuntimeStatus, TurnResult, WorkspaceFile,
};
use vtcode_webmcp::{
    FileChange, FilesystemWorkspace, PairingDisplay, WebmcpEventHub, WebmcpServer, WebmcpServerConfig,
};

const ACTIVE_PROMPT_QUEUE_CAPACITY: usize = 8;
const MAX_TURN_PROMPT_BYTES: usize = 16 * 1024;

/// A WebMCP bridge attached to the current interactive VT Code session.
///
/// The bridge owns only the transport task and its authenticated runtime
/// adapter. Dropping it revokes pairings and stops the listener, so a session
/// cannot leave a browser connection serving after the TUI exits.
pub(crate) struct ActiveWebmcpBridge {
    server: WebmcpServer,
    task: JoinHandle<()>,
    endpoint: String,
    pairing: PairingDisplay,
}

impl ActiveWebmcpBridge {
    /// Start an active-session bridge for one explicitly allowed browser origin.
    pub(crate) async fn start(
        workspace: &Path,
        config: Option<&VTCodeConfig>,
        origin: &str,
        prompt_sender: mpsc::Sender<String>,
    ) -> Result<Self> {
        let settings = config.map_or_else(WebmcpConfig::default, |config| config.webmcp.clone());
        let allowed_origins = configured_origins(&settings, origin)?;
        let workspace = FilesystemWorkspace::new(workspace, [workspace.to_path_buf()], false)
            .await
            .context("failed to initialize the active WebMCP workspace")?
            .with_checks_allowed(false);
        let adapter = ActiveRuntimeAdapter { workspace, prompt_sender };
        let server = WebmcpServer::new(
            Arc::new(adapter),
            WebmcpServerConfig {
                host: settings.host,
                port: settings.port,
                allowed_origins,
                pairing_ttl_secs: settings.pairing_ttl_secs,
                max_frame_bytes: settings.max_frame_bytes,
                max_in_flight_requests: settings.max_in_flight_requests,
                ..Default::default()
            },
        )?;
        let pairing = server.begin_pairing_for_origin(origin.to_string())?;
        let listener = server.bind().await.context("failed to bind the active WebMCP listener")?;
        let address = listener
            .local_addr()
            .context("failed to determine the active WebMCP listener address")?;
        let task_server = server.clone();
        let task = tokio::spawn(async move {
            if let Err(error) = task_server.serve_listener(listener).await {
                tracing::error!(error = %error, "active WebMCP listener stopped unexpectedly");
            }
        });

        Ok(Self {
            server,
            task,
            endpoint: format!("ws://{address}/webmcp"),
            pairing,
        })
    }

    /// The WebSocket endpoint to enter in the browser editor.
    pub(crate) fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// The one-time pairing code to enter in the browser editor.
    pub(crate) fn pairing_code(&self) -> &str {
        self.pairing.code()
    }

    /// Remaining lifetime of the one-time pairing code.
    pub(crate) fn pairing_expires_in_secs(&self) -> u64 {
        self.pairing.expires_in().as_secs()
    }

    /// Event hub receiving canonical runtime events for this bridge.
    pub(crate) fn event_hub(&self) -> WebmcpEventHub {
        self.server.event_hub()
    }
}

impl Drop for ActiveWebmcpBridge {
    fn drop(&mut self) {
        self.server.revoke_all_pairings();
        self.task.abort();
    }
}

fn configured_origins(settings: &WebmcpConfig, requested_origin: &str) -> Result<Vec<String>> {
    if settings.allowed_origins.is_empty() {
        return Ok(vec![requested_origin.to_string()]);
    }
    if !settings.allowed_origins.iter().any(|origin| origin == requested_origin) {
        bail!("origin {requested_origin} is not present in [webmcp].allowed_origins");
    }
    Ok(settings.allowed_origins.clone())
}

#[derive(Clone)]
struct ActiveRuntimeAdapter {
    workspace: FilesystemWorkspace,
    prompt_sender: mpsc::Sender<String>,
}

#[async_trait]
impl RuntimeAdapter for ActiveRuntimeAdapter {
    async fn status(&self) -> vtcode_webmcp::Result<RuntimeStatus> {
        let mut status = self.workspace.status().await?;
        status.turns_available = true;
        status.approval_authority = "active VT Code terminal".into();
        Ok(status)
    }

    async fn list_files(&self) -> vtcode_webmcp::Result<Vec<WorkspaceFile>> {
        self.workspace.list_files().await
    }

    async fn read_file(&self, path: &str) -> vtcode_webmcp::Result<FileSnapshot> {
        self.workspace.read_file(path).await
    }

    async fn propose_changes(&self, changes: Vec<FileChange>) -> vtcode_webmcp::Result<PatchProposal> {
        self.workspace.propose_changes(changes).await
    }

    async fn apply_proposal(&self, proposal_id: &str) -> vtcode_webmcp::Result<AppliedChange> {
        let _ = proposal_id;
        Err(vtcode_webmcp::WebmcpError::ApprovalRequired)
    }

    async fn run_checks(&self, command: &str) -> vtcode_webmcp::Result<CheckResult> {
        let _ = command;
        Err(vtcode_webmcp::WebmcpError::ApprovalRequired)
    }

    async fn revert_last_change(&self, change_id: &str) -> vtcode_webmcp::Result<AppliedChange> {
        let _ = change_id;
        Err(vtcode_webmcp::WebmcpError::ApprovalRequired)
    }

    async fn request_turn(&self, prompt: &str) -> vtcode_webmcp::Result<TurnResult> {
        if prompt.trim().is_empty() {
            return Err(vtcode_webmcp::WebmcpError::InvalidRequest("agent turn prompt cannot be empty".to_string()));
        }
        if prompt.len() > MAX_TURN_PROMPT_BYTES {
            return Err(vtcode_webmcp::WebmcpError::LimitExceeded);
        }
        self.prompt_sender.try_send(prompt.to_string()).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => vtcode_webmcp::WebmcpError::LimitExceeded,
            mpsc::error::TrySendError::Closed(_) => {
                vtcode_webmcp::WebmcpError::Unsupported("the active VT Code session has ended".to_string())
            }
        })?;
        Ok(TurnResult {
            turn_id: format!("webmcp-{}", Uuid::new_v4().simple()),
            accepted: true,
        })
    }
}

/// Create the bounded prompt channel used between the WebMCP adapter and the
/// active interaction loop.
pub(crate) fn prompt_channel() -> (mpsc::Sender<String>, mpsc::Receiver<String>) {
    mpsc::channel(ACTIVE_PROMPT_QUEUE_CAPACITY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn active_adapter_reports_runtime_and_queues_turns() {
        let workspace_root = tempdir().expect("workspace");
        let workspace = FilesystemWorkspace::new(workspace_root.path(), [workspace_root.path().to_path_buf()], false)
            .await
            .expect("filesystem workspace");
        let (prompt_sender, mut prompt_receiver) = prompt_channel();
        let adapter = ActiveRuntimeAdapter { workspace, prompt_sender };

        let status = adapter.status().await.expect("runtime status");
        assert!(status.connected);
        assert!(status.turns_available);
        assert!(!status.mutations_allowed);
        assert_eq!(status.approval_authority, "active VT Code terminal");

        let result = adapter.request_turn("review this draft").await.expect("turn request");
        assert!(result.accepted);
        assert_eq!(prompt_receiver.recv().await.as_deref(), Some("review this draft"));
    }

    #[tokio::test]
    async fn active_adapter_rejects_empty_turns() {
        let workspace_root = tempdir().expect("workspace");
        let workspace = FilesystemWorkspace::new(workspace_root.path(), [], false)
            .await
            .expect("filesystem workspace");
        let (prompt_sender, _prompt_receiver) = prompt_channel();
        let adapter = ActiveRuntimeAdapter { workspace, prompt_sender };

        assert!(matches!(adapter.request_turn("  ").await, Err(vtcode_webmcp::WebmcpError::InvalidRequest(_))));
    }
}
