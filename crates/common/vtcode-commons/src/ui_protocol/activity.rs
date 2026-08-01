//! Explicit global activity states shared by the runloop and terminal UI.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ActivityState {
    #[default]
    Idle,
    /// The agent is working inside the plan workflow (research/synthesis).
    Planning,
    PreparingFreshExecutionThread,
    RestoringApprovedPlan,
    /// The agent is executing an approved plan.
    Building,
    StartingBuild,
}

impl ActivityState {
    /// Transient handoff states that must block input and mode switches.
    pub const fn is_busy(self) -> bool {
        matches!(self, Self::PreparingFreshExecutionThread | Self::RestoringApprovedPlan | Self::StartingBuild)
    }

    /// Long-lived display stages that keep input enabled between turns.
    pub const fn is_stage(self) -> bool {
        matches!(self, Self::Planning | Self::Building)
    }

    pub const fn status(self) -> Option<&'static str> {
        match self {
            Self::Idle => None,
            Self::Planning => Some("Planning..."),
            Self::PreparingFreshExecutionThread => Some("Preparing fresh execution thread..."),
            Self::RestoringApprovedPlan => Some("Restoring approved plan..."),
            Self::Building => Some("Building..."),
            Self::StartingBuild => Some("Starting build..."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ActivityState;

    #[test]
    fn idle_has_no_status_and_is_not_busy_or_stage() {
        assert_eq!(ActivityState::Idle.status(), None);
        assert!(!ActivityState::Idle.is_busy());
        assert!(!ActivityState::Idle.is_stage());
    }

    #[test]
    fn stages_are_not_busy_but_keep_a_status() {
        for state in [ActivityState::Planning, ActivityState::Building] {
            assert!(!state.is_busy());
            assert!(state.is_stage());
            assert!(state.status().is_some());
        }
        assert_eq!(ActivityState::Planning.status(), Some("Planning..."));
        assert_eq!(ActivityState::Building.status(), Some("Building..."));
    }

    #[test]
    fn transient_handoffs_are_busy_but_not_stages() {
        for state in [
            ActivityState::PreparingFreshExecutionThread,
            ActivityState::RestoringApprovedPlan,
            ActivityState::StartingBuild,
        ] {
            assert!(state.is_busy());
            assert!(!state.is_stage());
            assert!(state.status().is_some());
        }
    }
}
