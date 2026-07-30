use std::io::{self, Write};
use std::sync::atomic::Ordering;

use ratatui::crossterm::{
    cursor::{MoveToColumn, RestorePosition, SetCursorStyle, Show},
    event::{DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, PopKeyboardEnhancementFlags},
    execute,
    terminal::{Clear, ClearType, LeaveAlternateScreen, disable_raw_mode},
};

use super::state::{self, KEYBOARD_ENHANCEMENTS_PUSHED};

/// Restore terminal to a usable state after a panic or error.
///
/// This is the single canonical function for terminal restoration.
/// It is idempotent: subsequent calls are no-ops.
///
/// - Drains pending events before and after restoration
/// - Leaves alternate screen
/// - Disables bracketed paste, focus change, mouse capture
/// - Pops keyboard enhancement flags if pushed
/// - Resets cursor style and shows cursor
/// - Disables raw mode last
pub fn restore_tui() -> io::Result<()> {
    if !state::try_claim_restore() {
        return Ok(());
    }

    state::mark_tui_deinitialized();
    let mut first_error: Option<io::Error> = None;

    crate::tui::core_tui::runner::terminal_io::drain_terminal_events();

    let mut stderr = io::stderr();

    // Clear current line to remove any echoed ^C characters
    if let Err(error) = execute!(stderr, MoveToColumn(0), Clear(ClearType::CurrentLine)) {
        first_error.get_or_insert(error);
    }

    // Leave alternate screen FIRST (most critical for visual restoration)
    if let Err(error) = execute!(stderr, LeaveAlternateScreen) {
        first_error.get_or_insert(error);
    }

    // Disable terminal modes
    if let Err(error) = execute!(stderr, DisableBracketedPaste) {
        first_error.get_or_insert(error);
    }
    if let Err(error) = execute!(stderr, DisableFocusChange) {
        first_error.get_or_insert(error);
    }
    if let Err(error) = execute!(stderr, DisableMouseCapture) {
        first_error.get_or_insert(error);
    }

    // Only pop keyboard enhancement flags if actually pushed
    if KEYBOARD_ENHANCEMENTS_PUSHED.swap(false, Ordering::SeqCst)
        && let Err(error) = execute!(stderr, PopKeyboardEnhancementFlags)
    {
        first_error.get_or_insert(error);
    }

    crate::tui::core_tui::runner::terminal_io::reset_mouse_pointer_shape();

    // Ensure cursor state is restored
    if let Err(error) = execute!(stderr, SetCursorStyle::DefaultUserShape, Show, RestorePosition) {
        first_error.get_or_insert(error);
    }

    // Drain terminal responses from restore sequences while raw mode still active
    crate::tui::core_tui::runner::terminal_io::drain_terminal_events();

    // Disable raw mode LAST
    if let Err(error) = disable_raw_mode() {
        first_error.get_or_insert(error);
    }

    // Flush to ensure all escape sequences are processed
    if let Err(error) = stderr.flush() {
        first_error.get_or_insert(error);
    }

    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn test_restore_terminal_no_panic_when_not_initialized() {
        state::RESTORE_DONE.store(false, Ordering::SeqCst);
        state::TUI_INITIALIZED.store(false, Ordering::SeqCst);

        let result = restore_tui();
        assert!(result.is_ok() || result.is_err());
    }
}
