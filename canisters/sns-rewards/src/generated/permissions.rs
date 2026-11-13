// Generated from official governance.proto at 98c898f8de068a1fc0e97ebc805f9fa7f315d862.
// Source SHA-256: c8135574513ec10a9542079655b3fc85e0033f7c7b689bf0ab33a527927179f8
// Candid represents these protobuf enum values as int32.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum NeuronPermissionType {
    Unspecified = 0,
    ConfigureDissolveState = 1,
    ManagePrincipals = 2,
    SubmitProposal = 3,
    Vote = 4,
    Disburse = 5,
    Split = 6,
    MergeMaturity = 7,
    DisburseMaturity = 8,
    StakeMaturity = 9,
    ManageVotingPermission = 10,
}
impl TryFrom<i32> for NeuronPermissionType {
    type Error = ();
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Unspecified),
            1 => Ok(Self::ConfigureDissolveState),
            2 => Ok(Self::ManagePrincipals),
            3 => Ok(Self::SubmitProposal),
            4 => Ok(Self::Vote),
            5 => Ok(Self::Disburse),
            6 => Ok(Self::Split),
            7 => Ok(Self::MergeMaturity),
            8 => Ok(Self::DisburseMaturity),
            9 => Ok(Self::StakeMaturity),
            10 => Ok(Self::ManageVotingPermission),
            _ => Err(()),
        }
    }
}

pub const GOVERNANCE_ERROR_NOT_FOUND: i32 = 3;
