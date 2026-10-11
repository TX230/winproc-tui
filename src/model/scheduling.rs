#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PriorityClass {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
    Realtime,
    Unknown(u32),
}

impl PriorityClass {
    pub(crate) const EDITABLE: [Self; 5] = [
        Self::Idle,
        Self::BelowNormal,
        Self::Normal,
        Self::AboveNormal,
        Self::High,
    ];

    pub(crate) fn label(self) -> String {
        match self {
            Self::Idle => "Idle".into(),
            Self::BelowNormal => "Below normal".into(),
            Self::Normal => "Normal".into(),
            Self::AboveNormal => "Above normal".into(),
            Self::High => "High".into(),
            Self::Realtime => "Realtime".into(),
            Self::Unknown(value) => format!("Unknown (0x{value:X})"),
        }
    }

    pub(crate) fn editable(self) -> bool {
        Self::EDITABLE.contains(&self)
    }

    pub(crate) fn native(self) -> u32 {
        match self {
            Self::Idle => 0x40,
            Self::BelowNormal => 0x4000,
            Self::Normal => 0x20,
            Self::AboveNormal => 0x8000,
            Self::High => 0x80,
            Self::Realtime => 0x100,
            Self::Unknown(value) => value,
        }
    }

    pub(crate) fn from_native(value: u32) -> Self {
        Self::EDITABLE
            .into_iter()
            .chain([Self::Realtime])
            .find(|class| class.native() == value)
            .unwrap_or(Self::Unknown(value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PriorityChange {
    pub(crate) expected: PriorityClass,
    pub(crate) desired: PriorityClass,
    pub(crate) restore: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PriorityOutcome {
    Read,
    Changed,
    Restored,
    Failed(String),
    AppliedUnverified(String),
}

#[derive(Debug, Clone)]
pub(crate) struct PriorityReport {
    pub(crate) current: Option<PriorityClass>,
    pub(crate) writable: bool,
    pub(crate) restore: Option<PriorityChange>,
    pub(crate) outcome: PriorityOutcome,
}

impl PriorityReport {
    pub(crate) fn unavailable(message: String) -> Self {
        Self {
            current: None,
            writable: false,
            restore: None,
            outcome: PriorityOutcome::Failed(message),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessQos {
    SystemManaged,
    Eco,
    Disabled,
}
impl ProcessQos {
    pub(crate) fn from_masks(control: u32, state: u32) -> Self {
        if control & 1 == 0 {
            Self::SystemManaged
        } else if state & 1 != 0 {
            Self::Eco
        } else {
            Self::Disabled
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::SystemManaged => "System managed",
            Self::Eco => "Enabled",
            Self::Disabled => "Explicitly disabled",
        }
    }
}
#[cfg(test)]
mod qos_tests {
    use super::ProcessQos;
    #[test]
    fn execution_speed_policy_uses_control_and_state_masks() {
        assert_eq!(ProcessQos::from_masks(0, 0), ProcessQos::SystemManaged);
        assert_eq!(ProcessQos::from_masks(0, 1), ProcessQos::SystemManaged);
        assert_eq!(ProcessQos::from_masks(1, 1), ProcessQos::Eco);
        assert_eq!(ProcessQos::from_masks(1, 0), ProcessQos::Disabled);
        assert_eq!(ProcessQos::from_masks(2, 2), ProcessQos::SystemManaged);
    }
}
