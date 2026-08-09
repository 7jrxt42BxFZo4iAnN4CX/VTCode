use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use chrono::Local;

use vtcode_core::config::constants::ui;
use vtcode_core::config::{StatusLineConfig, StatusLineMode};
use vtcode_core::llm::provider::LLMProvider;
use vtcode_core::models::ModelId;
use vtcode_ui::tui::app::InlineHandle;

use super::status_line_command::run_status_line_command;
use crate::agent::runloop::git::{GitStatusSummary, git_status_summary};
use vtcode_core::llm::providers::clean_reasoning_text;
#[derive(Default, Clone)]
pub(crate) struct InputStatusState {
    pub(crate) left: Option<String>,
    pub(crate) right: Option<String>,
    pub(crate) git_left: Option<String>,
    pub(crate) git_summary: Option<GitStatusSummary>,
    pub(crate) last_git_refresh: Option<Instant>,
    pub(crate) command_value: Option<String>,
    pub(crate) last_command_refresh: Option<Instant>,
    // Context usage metrics
    pub(crate) context_utilization: Option<f64>,
    pub(crate) context_tokens: Option<usize>,
    pub(crate) context_limit_tokens: Option<usize>,
    pub(crate) context_remaining_tokens: Option<usize>,
    pub(crate) is_cancelling: bool,
    pub(crate) clock: Option<String>,
    // Dynamic context discovery status
    pub(crate) spooled_files_count: Option<usize>,
    pub(crate) thread_context: Option<String>,
    // Balance & cost info (shown for DeepSeek and OpenAI providers)
    pub(crate) show_costs: bool,
    pub(crate) balance: Option<String>,
    pub(crate) cost_usd: Option<f64>,
    pub(crate) cache_hit_pct: Option<f64>,
    pub(crate) last_balance_refresh: Option<Instant>,
    /// Set to `true` whenever a status-affecting field actually changes.
    /// The interaction loop clears it after a successful status rebuild and
    /// uses it to skip `update_input_status_if_changed` when nothing changed
    /// and the clock second hasn't ticked.
    pub(crate) status_dirty: bool,
    /// The Unix-second of the last formatted clock value. Used to detect when
    /// the `HH:MM:SS` display would actually change (once per second) so the
    /// status rebuild can be skipped between ticks.
    pub(crate) last_clock_second: Option<u64>,
}

const GIT_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Default)]
struct BottomStatusLayout {
    left: Vec<String>,
    right: Vec<String>,
}

impl BottomStatusLayout {
    fn push_left(&mut self, value: Option<String>) {
        if let Some(value) = value {
            push_unique(&mut self.left, value);
        }
    }

    fn into_status(self) -> (Option<String>, Option<String>) {
        (join_status_components(self.left), join_status_components(self.right))
    }
}

pub(crate) fn update_context_budget(state: &mut InputStatusState, used_tokens: usize, limit_tokens: usize) {
    if limit_tokens == 0 {
        if state.context_utilization.is_some() {
            state.context_utilization = None;
            state.context_tokens = None;
            state.context_limit_tokens = None;
            state.context_remaining_tokens = None;
            state.status_dirty = true;
        }
        return;
    }

    let used = used_tokens.min(limit_tokens);
    let remaining = limit_tokens.saturating_sub(used);
    let left_percent = (remaining as f64 / limit_tokens as f64) * 100.0;
    let utilization = Some(left_percent.clamp(0.0, 100.0));

    if state.context_utilization != utilization
        || state.context_tokens != Some(used)
        || state.context_limit_tokens != Some(limit_tokens)
        || state.context_remaining_tokens != Some(remaining)
    {
        state.context_utilization = utilization;
        state.context_tokens = Some(used);
        state.context_limit_tokens = Some(limit_tokens);
        state.context_remaining_tokens = Some(remaining);
        state.status_dirty = true;
    }
}

pub(crate) fn status_line_shows_auto_components(status_config: Option<&StatusLineConfig>) -> bool {
    match status_config.map(|cfg| cfg.effective_mode()).unwrap_or(StatusLineMode::Auto) {
        StatusLineMode::Auto | StatusLineMode::Unknown => true,
        StatusLineMode::Command => status_config
            .and_then(|cfg| cfg.command.as_deref())
            .map(|command| command.trim().is_empty())
            .unwrap_or(true),
        StatusLineMode::Hidden => false,
    }
}

pub(crate) fn status_line_shows_clock(status_config: Option<&StatusLineConfig>) -> bool {
    status_config.map(|cfg| cfg.show_clock).unwrap_or(true)
}

fn now_statusline_clock() -> String {
    Local::now().format("%H:%M:%S").to_string()
}

/// Cheap `SystemTime` read returning the current Unix second without any
/// timezone resolution or string allocation. Used to detect whether the
/// `HH:MM:SS` clock display would actually differ from the last frame.
fn current_second() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Returns `true` when the clock display (HH:MM:SS) would change since the
/// last recorded second, updating the tracked second in the process.
pub(crate) fn clock_second_ticked(state: &mut InputStatusState) -> bool {
    let now = current_second();
    let changed = state.last_clock_second.is_none_or(|prev| prev != now);
    state.last_clock_second = Some(now);
    changed
}

pub(crate) async fn update_input_status_if_changed(
    handle: &InlineHandle,
    workspace: &Path,
    model: &str,
    reasoning: &str,
    status_config: Option<&StatusLineConfig>,
    state: &mut InputStatusState,
) -> Result<()> {
    // Get the effective status line mode, using defaults when config is unset
    // (following Codex PR #12015 pattern for default configuration fallback)
    let mode = status_config.map(|cfg| cfg.effective_mode()).unwrap_or(StatusLineMode::Auto);
    let status_line_hidden = matches!(mode, StatusLineMode::Hidden);

    if status_line_hidden {
        state.git_left = None;
    }

    let should_refresh_git = match state.last_git_refresh {
        Some(last_refresh) => last_refresh.elapsed() >= GIT_STATUS_REFRESH_INTERVAL,
        None => true,
    };

    if should_refresh_git {
        let workspace_buf = workspace.to_path_buf();
        match tokio::task::spawn_blocking(move || git_status_summary(&workspace_buf)).await? {
            Ok(Some(summary)) => {
                let indicator = if summary.dirty {
                    ui::HEADER_GIT_DIRTY_SUFFIX
                } else {
                    ui::HEADER_GIT_CLEAN_SUFFIX
                };
                let display = if summary.branch.is_empty() {
                    indicator.to_string()
                } else {
                    format!("{}{}", summary.branch, indicator)
                };
                state.git_left = (!status_line_hidden).then_some(display);
                state.git_summary = Some(summary);
            }
            Ok(None) => {
                state.git_left = None;
                state.git_summary = None;
            }
            Err(error) => {
                tracing::warn!(
                    workspace = %workspace.display(),
                    error = ?error,
                    "Failed to resolve git status"
                );
                state.git_summary = None;
                state.git_left = None;
            }
        }

        state.last_git_refresh = Some(Instant::now());
    }

    let trimmed_model = model.trim();
    let cleaned_reasoning = clean_reasoning_text(reasoning.trim());
    let trimmed_reasoning = cleaned_reasoning.as_str();
    let model_display = ModelId::from_str(trimmed_model)
        .map(|id| id.display_name().to_string())
        .unwrap_or_else(|_| trimmed_model.to_string());

    let mut command_error: Option<anyhow::Error> = None;

    state.clock = status_line_shows_clock(status_config).then(now_statusline_clock);

    let (left, right) = match mode {
        StatusLineMode::Hidden => {
            state.command_value = None;
            state.last_command_refresh = None;
            (None, None)
        }
        StatusLineMode::Auto | StatusLineMode::Unknown => auto_status_layout(state).into_status(),
        StatusLineMode::Command => {
            if let Some(cfg) = status_config {
                if let Some(command) = cfg
                    .command
                    .as_ref()
                    .map(|cmd| cmd.trim().to_string())
                    .filter(|cmd| !cmd.is_empty())
                {
                    let refresh_interval = Duration::from_millis(cfg.refresh_interval_ms);
                    let should_refresh_command = match state.last_command_refresh {
                        Some(last_refresh) => {
                            if refresh_interval.is_zero() {
                                true
                            } else {
                                last_refresh.elapsed() >= refresh_interval
                            }
                        }
                        None => true,
                    };

                    if should_refresh_command {
                        state.last_command_refresh = Some(Instant::now());
                        match run_status_line_command(
                            &command,
                            workspace,
                            trimmed_model,
                            &model_display,
                            trimmed_reasoning,
                            state.git_summary.as_ref(),
                            cfg,
                        )
                        .await
                        {
                            Ok(output) => {
                                state.command_value = output;
                            }
                            Err(error) => {
                                command_error = Some(error);
                            }
                        }
                    }

                    (state.command_value.clone(), None)
                } else {
                    state.command_value = None;
                    auto_status_layout(state).into_status()
                }
            } else {
                state.command_value = None;
                auto_status_layout(state).into_status()
            }
        }
    };

    if state.left != left || state.right != right {
        handle.set_input_status(left.clone(), right.clone());
        state.left = left;
        state.right = right;
    }

    if let Some(error) = command_error {
        Err(error)
    } else {
        Ok(())
    }
}

fn auto_status_layout(state: &InputStatusState) -> BottomStatusLayout {
    let mut layout = BottomStatusLayout::default();
    layout.push_left(state.git_left.clone());
    layout.right =
        auto_status_components(state.thread_context.as_deref(), state.is_cancelling, state.spooled_files_count, state);
    layout
}

/// Build model status with all context indicators including spooled files
#[cfg(test)]
fn build_model_status_with_context_and_spooled(
    thread_context: Option<&str>,
    is_cancelling: bool,
    spooled_files: Option<usize>,
) -> Option<String> {
    let default_state = InputStatusState::default();
    join_status_components(auto_status_components(thread_context, is_cancelling, spooled_files, &default_state))
}

fn auto_status_components(
    thread_context: Option<&str>,
    is_cancelling: bool,
    spooled_files: Option<usize>,
    state: &InputStatusState,
) -> Vec<String> {
    let mut parts: Vec<Option<String>> = Vec::new();
    if is_cancelling {
        parts.push(Some("CANCELLING...".to_string()));
    }

    parts.push(
        thread_context
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    );

    // Provider-specific balance/cost/cache display (DeepSeek, OpenAI)
    if state.show_costs {
        if let Some(bal) = &state.balance {
            parts.push(Some(format!("Balance: {bal}")));
        }
        if let Some(cost) = state.cost_usd {
            parts.push(Some(format!("Cost: ${cost:.4}")));
        }
        if let Some(pct) = state.cache_hit_pct {
            parts.push(Some(format!("Cache: {pct:.0}%")));
        }
    }

    if let Some(count) = spooled_files
        && count > 0
    {
        parts.push(Some(format!("{count} spooled")));
    }

    // Current system time, pinned to the right edge of the status line.
    parts.push(state.clock.clone());

    let mut deduped = Vec::new();
    for value in parts.into_iter().flatten() {
        push_unique(&mut deduped, value);
    }
    deduped
}

fn push_unique(values: &mut Vec<String>, value: String) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }
    let normalized = normalize_for_dedup(trimmed);
    if values.iter().any(|existing| normalize_for_dedup(existing) == normalized) {
        return;
    }
    values.push(trimmed.to_string());
}

fn normalize_for_dedup(value: &str) -> String {
    value.split_whitespace().fold(String::new(), |mut acc, s| {
        if !acc.is_empty() {
            acc.push(' ');
        }
        acc.push_str(s);
        acc
    })
}

fn join_status_components(values: Vec<String>) -> Option<String> {
    let parts = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

/// Update spooled files count in status state
pub(crate) fn update_spooled_files_count(state: &mut InputStatusState, count: usize) {
    if state.spooled_files_count != Some(count) {
        state.spooled_files_count = Some(count);
        state.status_dirty = true;
    }
}

pub(crate) fn update_thread_context(state: &mut InputStatusState, thread_label: &str, local_agent_count: usize) {
    let mut value = thread_label.trim().to_string();
    if local_agent_count > 0 {
        let suffix = format!("{} agent{}", local_agent_count, if local_agent_count == 1 { "" } else { "s" });
        if value.is_empty() {
            value = suffix;
        } else {
            value.push_str(" · ");
            value.push_str(&suffix);
        }
    }
    let new_value = (!value.trim().is_empty()).then_some(value);
    if state.thread_context != new_value {
        state.thread_context = new_value;
        state.status_dirty = true;
    }
}

/// Refresh account balance from the provider and update the status line.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn refresh_balance_info(
    provider_client: &dyn LLMProvider,
    handle: &InlineHandle,
    workspace: &Path,
    model: &str,
    reasoning_effort: &str,
    status_config: Option<&StatusLineConfig>,
    state: &mut InputStatusState,
) {
    const BALANCE_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
    const BALANCE_REQUEST_TIMEOUT: Duration = Duration::from_millis(750);

    let stale = state
        .last_balance_refresh
        .map(|t| t.elapsed() >= BALANCE_REFRESH_INTERVAL)
        .unwrap_or(true);
    if !stale {
        return;
    }

    state.last_balance_refresh = Some(Instant::now());

    match tokio::time::timeout(BALANCE_REQUEST_TIMEOUT, provider_client.get_balance()).await {
        Ok(Ok(Some(bal))) => {
            let warn = if bal.is_available { "" } else { " !" };
            let new_balance = Some(format!("{}{}", bal.display, warn));
            if state.balance != new_balance {
                state.balance = new_balance;
                state.status_dirty = true;
            }
            if let Err(e) =
                update_input_status_if_changed(handle, workspace, model, reasoning_effort, status_config, state).await
            {
                tracing::debug!("Failed to refresh status after balance fetch: {e}");
            }
        }
        Ok(Ok(None)) => {
            if state.balance.is_some() {
                state.balance = None;
                state.status_dirty = true;
            }
        }
        Ok(Err(e)) => {
            tracing::debug!("Failed to fetch provider balance: {e}");
        }
        Err(_) => {
            tracing::debug!("Timed out fetching provider balance");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        InputStatusState, build_model_status_with_context_and_spooled, status_line_shows_clock, update_thread_context,
    };
    use vtcode_core::config::StatusLineConfig;

    #[test]
    fn status_line_shows_thread_context() {
        let status = build_model_status_with_context_and_spooled(Some("main"), false, None);

        assert_eq!(status.as_deref(), Some("main"));
    }

    #[test]
    fn status_line_shows_spooled_files() {
        let status = build_model_status_with_context_and_spooled(None, false, Some(3));

        assert_eq!(status.as_deref(), Some("3 spooled"));
    }

    #[test]
    fn thread_context_keeps_zero_active_subagent_count_visible() {
        let mut state = InputStatusState::default();
        update_thread_context(&mut state, "main", 0);

        assert_eq!(state.thread_context.as_deref(), Some("main"));
    }

    #[test]
    fn thread_context_shows_compact_agent_badge_when_present() {
        let mut state = InputStatusState::default();
        update_thread_context(&mut state, "main", 2);

        assert_eq!(state.thread_context.as_deref(), Some("main · 2 agents"));
    }

    #[test]
    fn status_line_shows_clock_when_set() {
        let state = InputStatusState {
            clock: Some("14:30:05".to_string()),
            ..Default::default()
        };

        let components = super::auto_status_components(None, false, None, &state);

        assert_eq!(components, vec!["14:30:05"]);
    }

    #[test]
    fn status_line_clock_respects_show_clock_config() {
        assert!(status_line_shows_clock(None), "unset config defaults to showing the clock");

        let on = StatusLineConfig { show_clock: true, ..StatusLineConfig::default() };
        assert!(status_line_shows_clock(Some(&on)));

        let off = StatusLineConfig { show_clock: false, ..StatusLineConfig::default() };
        assert!(!status_line_shows_clock(Some(&off)));
    }

    #[test]
    fn clock_second_ticked_returns_true_on_first_call() {
        let mut state = InputStatusState::default();
        assert!(super::clock_second_ticked(&mut state), "first call must report a tick");
        assert!(state.last_clock_second.is_some(), "second should be tracked");
    }

    #[test]
    fn clock_second_ticked_returns_false_within_same_second() {
        let mut state = InputStatusState::default();
        super::clock_second_ticked(&mut state);
        // A second call within the same second must not report a tick.
        assert!(!super::clock_second_ticked(&mut state), "same-second call must not tick");
    }

    #[test]
    fn update_spooled_files_count_sets_dirty_only_on_change() {
        let mut state = InputStatusState::default();
        super::update_spooled_files_count(&mut state, 3);
        assert!(state.status_dirty, "first set must mark dirty");

        state.status_dirty = false;
        super::update_spooled_files_count(&mut state, 3);
        assert!(!state.status_dirty, "unchanged value must not mark dirty");

        super::update_spooled_files_count(&mut state, 5);
        assert!(state.status_dirty, "changed value must mark dirty");
    }

    #[test]
    fn update_context_budget_sets_dirty_only_on_change() {
        let mut state = InputStatusState::default();
        super::update_context_budget(&mut state, 1000, 10_000);
        assert!(state.status_dirty, "first set must mark dirty");

        state.status_dirty = false;
        super::update_context_budget(&mut state, 1000, 10_000);
        assert!(!state.status_dirty, "unchanged values must not mark dirty");

        super::update_context_budget(&mut state, 2000, 10_000);
        assert!(state.status_dirty, "changed used tokens must mark dirty");
    }
}
