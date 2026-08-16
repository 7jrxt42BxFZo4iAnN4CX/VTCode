use std::io;

use anyhow::Result;
use ratatui::crossterm::{
    cursor::{RestorePosition, SavePosition},
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste, EnableFocusChange,
        EnableMouseCapture,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

use crate::tui::options::FullscreenInteractionSettings;

/// Represents the state of terminal modes before TUI initialization.
///
/// This struct tracks which terminal features were enabled before we
/// modified them, allowing proper restoration on exit.
#[derive(Debug, Clone)]
pub(super) struct TerminalModeState {
    /// Whether bracketed paste was enabled (we enable it)
    bracketed_paste_enabled: bool,
    /// Whether raw mode was enabled (we enable it)
    raw_mode_enabled: bool,
    /// Whether mouse capture was enabled (we enable it)
    mouse_capture_enabled: bool,
    /// Whether focus change events were enabled (we enable them)
    focus_change_enabled: bool,
    /// Whether keyboard enhancement flags were pushed (we push them)
    keyboard_enhancements_pushed: bool,
    /// Whether the cursor position was saved before entering fullscreen
    cursor_position_saved: bool,
    /// Whether the alternate screen buffer is active
    alternate_screen_active: bool,
}

impl TerminalModeState {
    /// Create a new TerminalModeState with all modes disabled (clean state)
    fn new() -> Self {
        Self {
            bracketed_paste_enabled: false,
            raw_mode_enabled: false,
            mouse_capture_enabled: false,
            focus_change_enabled: false,
            keyboard_enhancements_pushed: false,
            cursor_position_saved: false,
            alternate_screen_active: false,
        }
    }

    pub(super) fn save_cursor_position(&mut self, stderr: &mut io::Stderr) {
        match execute!(stderr, SavePosition) {
            Ok(_) => self.cursor_position_saved = true,
            Err(error) => {
                tracing::debug!(%error, "failed to save cursor position for inline session");
            }
        }
    }

    pub(super) fn enter_alternate_screen(&mut self, stderr: &mut io::Stderr) -> Result<()> {
        execute!(stderr, EnterAlternateScreen)
            .map_err(|error| anyhow::anyhow!("failed to enter alternate inline screen: {error}"))?;
        self.alternate_screen_active = true;
        Ok(())
    }

    pub(super) fn push_keyboard_enhancement_flags(
        &mut self,
        stderr: &mut io::Stderr,
        keyboard_flags: crossterm::event::KeyboardEnhancementFlags,
    ) {
        use ratatui::crossterm::event::PushKeyboardEnhancementFlags;

        if keyboard_flags.is_empty() {
            return;
        }

        match execute!(stderr, PushKeyboardEnhancementFlags(keyboard_flags)) {
            Ok(_) => {
                self.keyboard_enhancements_pushed = true;
                crate::tui::ui::tui::panic_hook::mark_keyboard_enhancements_pushed(true);
            }
            Err(error) => {
                tracing::debug!(%error, "failed to push keyboard enhancement flags");
            }
        }
    }
}

impl Default for TerminalModeState {
    fn default() -> Self {
        Self::new()
    }
}

pub(super) fn enable_terminal_modes(
    stderr: &mut io::Stderr,
    fullscreen: &FullscreenInteractionSettings,
) -> Result<TerminalModeState> {
    let mut state = TerminalModeState::new();

    // Enable bracketed paste
    match execute!(stderr, EnableBracketedPaste) {
        Ok(_) => state.bracketed_paste_enabled = true,
        Err(error) => {
            tracing::warn!(%error, "failed to enable bracketed paste");
        }
    }

    // Enable raw mode
    match enable_raw_mode() {
        Ok(_) => state.raw_mode_enabled = true,
        Err(error) => {
            return Err(anyhow::anyhow!("failed to enable raw mode: {error}"));
        }
    }
    crate::tui::ui::tui::panic_hook::mark_terminal_modified();

    if fullscreen.mouse_capture {
        match execute!(stderr, EnableMouseCapture) {
            Ok(_) => state.mouse_capture_enabled = true,
            Err(error) => {
                tracing::warn!(%error, "failed to enable mouse capture");
            }
        }
    }

    // Enable focus change events
    match execute!(stderr, EnableFocusChange) {
        Ok(_) => state.focus_change_enabled = true,
        Err(error) => {
            tracing::debug!(%error, "failed to enable focus change events");
        }
    }

    Ok(state)
}

/// Restore terminal modes using the canonical single-source-of-truth function.
///
/// Delegates to `restore_tui()` which handles all terminal restoration
/// and is guarded by a `RESTORE_DONE` flag for idempotency.
pub(super) fn restore_terminal_modes(_state: &TerminalModeState) -> Result<()> {
    crate::tui::ui::tui::panic_hook::restore_tui()?;
    Ok(())
}
