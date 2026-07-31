//! Explicit global activity states shared by the runloop and terminal UI.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ActivityState {
    #[default]
    Idle,
    PreparingFreshExecutionThread,
    RestoringApprovedPlan,
    StartingBuild,
}

impl ActivityState {
    pub const fn is_busy(self) -> bool {
        !matches!(self, Self::Idle)
    }

    pub const fn status(self) -> Option<&'static str> {
        match self {
            Self::Idle => None,
            Self::PreparingFreshExecutionThread => Some("Preparing fresh execution thread..."),
            Self::RestoringApprovedPlan => Some("Restoring approved plan..."),
            Self::StartingBuild => Some("Starting build..."),
        }
    }
}
