use super::action::InlineLoopAction;
use super::queue::InlineQueueState;
use super::state::InlineEventState;
use vtcode_ui::tui::app::SubmittedInput;

pub(crate) struct InlineInputProcessor<'a, 'state> {
    state: &'a mut InlineEventState<'state>,
}

impl<'a, 'state> InlineInputProcessor<'a, 'state> {
    pub(crate) fn new(state: &'a mut InlineEventState<'state>) -> Self {
        Self { state }
    }

    pub(crate) fn submit(self, input: SubmittedInput) -> InlineLoopAction {
        self.state.reset_interrupt_state();
        InlineLoopAction::Submit(input.trim_text())
    }

    pub(crate) fn submit_prompt(self, input: SubmittedInput) -> InlineLoopAction {
        self.state.reset_interrupt_state();
        InlineLoopAction::SubmitPrompt(input.trim_text())
    }

    pub(crate) fn queue_submit(
        self,
        input: SubmittedInput,
        queue: &mut InlineQueueState<'_>,
        primary_agent: Option<String>,
    ) -> InlineLoopAction {
        self.state.reset_interrupt_state();
        let input = input.trim_text();
        if input.is_empty() {
            return InlineLoopAction::Continue;
        }

        queue.push(input, primary_agent);
        InlineLoopAction::Continue
    }

    pub(crate) fn passive(self) -> InlineLoopAction {
        self.state.reset_interrupt_state();
        InlineLoopAction::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runloop::unified::inline_events::interrupts::InlineInterruptCoordinator;
    use crate::agent::runloop::unified::inline_events::state::InlineEventState;
    use crate::agent::runloop::unified::state::CtrlCState;
    use vtcode_core::utils::ansi::AnsiRenderer;
    use vtcode_ui::tui::app::InlineHandle;

    #[test]
    fn webmcp_prompt_keeps_slash_commands_as_prompt_text() {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
        let ctrl_c_state = CtrlCState::new();
        let interrupts = InlineInterruptCoordinator::new(&ctrl_c_state);
        let mut ctrl_c_notice_displayed = false;
        let mut state = InlineEventState::new(&mut renderer, interrupts, &mut ctrl_c_notice_displayed);

        let action = InlineInputProcessor::new(&mut state).submit_prompt("/exit".into());

        assert!(matches!(action, InlineLoopAction::SubmitPrompt(input) if input.text == "/exit"));
    }
}
