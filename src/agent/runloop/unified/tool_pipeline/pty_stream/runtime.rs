use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anstyle::Color;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
use vtcode_core::config::PtyConfig;
use vtcode_core::tools::registry::ToolProgressCallback;
use vtcode_ui::tui::app::{InlineHandle, InlineMessageKind};

use crate::agent::runloop::unified::progress::ProgressReporter;

use super::state::PtyStreamState;

enum PtyStreamMessage {
    Output(String),
}

pub(crate) struct PtyStreamRuntime {
    sender: Option<mpsc::Sender<PtyStreamMessage>>,
    finish_sender: Option<oneshot::Sender<Color>>,
    task: Option<JoinHandle<()>>,
    active: Arc<AtomicBool>,
}

async fn process_output(
    handle: &InlineHandle,
    progress_reporter: &ProgressReporter,
    state: &mut PtyStreamState,
    output: String,
    tail_limit: usize,
) {
    if output.is_empty() {
        return;
    }

    state.apply_chunk(&output, tail_limit);
    let visible_output = vtcode_core::utils::ansi_parser::strip_ansi(&output);
    if visible_output.trim().is_empty() {
        return;
    }

    let (replace_count, segments, link_ranges, last_line) = state.render_current_segments(tail_limit);
    if !segments.is_empty() {
        handle.replace_last_with_links(replace_count, InlineMessageKind::Pty, segments, link_ranges);
    }

    if let Some(last_line) = last_line {
        let cleaned_last_line = vtcode_core::utils::ansi_parser::strip_ansi(&last_line);
        if !cleaned_last_line.trim().is_empty() {
            progress_reporter.set_message(cleaned_last_line).await;
        }
    }
}

impl PtyStreamRuntime {
    const MAX_LIVE_STREAM_LINES: usize = 12;

    pub(crate) fn start(
        handle: InlineHandle,
        progress_reporter: ProgressReporter,
        tail_limit: usize,
        command_prompt: Option<String>,
        pty_config: PtyConfig,
        workspace_root: Option<&Path>,
    ) -> (Self, ToolProgressCallback) {
        let owned_root = workspace_root.map(Path::to_path_buf);
        let (tx, mut rx) = mpsc::channel::<PtyStreamMessage>(256);
        let (finish_tx, mut finish_rx) = oneshot::channel::<Color>();
        let active = Arc::new(AtomicBool::new(true));
        let worker_active = Arc::clone(&active);
        let effective_tail_limit = tail_limit.clamp(1, Self::MAX_LIVE_STREAM_LINES);

        let task = tokio::spawn(async move {
            let mut state = PtyStreamState::new(command_prompt, pty_config, owned_root.as_deref());
            let (replace_count, segments, link_ranges, _) = state.render_segments("", effective_tail_limit);
            if !segments.is_empty() && worker_active.load(Ordering::Relaxed) {
                handle.replace_last_with_links(replace_count, InlineMessageKind::Pty, segments, link_ranges);
            }

            let mut finish_requested = None;
            loop {
                if !worker_active.load(Ordering::Relaxed) {
                    break;
                }

                if let Some(final_color) = finish_requested.take() {
                    // The status channel is deliberately separate from output,
                    // so a busy stream cannot strand the final status behind a
                    // full output queue. Drain output accepted before shutdown
                    // before applying the final color to the complete block.
                    while let Ok(message) = rx.try_recv() {
                        let PtyStreamMessage::Output(output) = message;
                        process_output(&handle, &progress_reporter, &mut state, output, effective_tail_limit).await;
                    }

                    state.set_header_color(final_color);
                    let (replace_count, segments, link_ranges, _) = state.render_current_segments(effective_tail_limit);
                    if !segments.is_empty() {
                        handle.replace_last_with_links(replace_count, InlineMessageKind::Pty, segments, link_ranges);
                    }
                    break;
                }

                tokio::select! {
                    biased;
                    result = &mut finish_rx => {
                        let Ok(color) = result else {
                            break;
                        };
                        finish_requested = Some(color);
                    }
                    message = rx.recv() => {
                        match message {
                            Some(PtyStreamMessage::Output(output)) => {
                                process_output(
                                    &handle,
                                    &progress_reporter,
                                    &mut state,
                                    output,
                                    effective_tail_limit,
                                )
                                .await;
                            }
                            None => break,
                        }
                    }
                }
            }
        });

        let callback_active = Arc::clone(&active);
        let callback_tx = tx.clone();
        let callback: ToolProgressCallback = Arc::new(move |_name: &str, output: &str| {
            if !callback_active.load(Ordering::Relaxed) || output.is_empty() {
                return;
            }
            let _ = callback_tx.try_send(PtyStreamMessage::Output(output.to_string()));
        });

        (
            Self {
                sender: Some(tx),
                finish_sender: Some(finish_tx),
                task: Some(task),
                active,
            },
            callback,
        )
    }

    pub(crate) async fn shutdown(mut self, header_color: Color) {
        let _ = self.sender.take();
        if let Some(finish_sender) = self.finish_sender.take() {
            let _ = finish_sender.send(header_color);
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
        self.active.store(false, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(crate) fn for_test(task: JoinHandle<()>, active: Arc<AtomicBool>) -> Self {
        Self {
            sender: None,
            finish_sender: None,
            task: Some(task),
            active,
        }
    }
}

impl Drop for PtyStreamRuntime {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Relaxed);
        let _ = self.sender.take();
        let _ = self.finish_sender.take();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
