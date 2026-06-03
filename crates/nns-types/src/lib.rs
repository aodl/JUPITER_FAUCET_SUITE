//! NNS Governance Candid wire types used by Jupiter.
//!
//! These committed DTOs are generated from the pinned Governance subset DID
//! under `candid/nns-governance/`. Production canister builds include plain Rust
//! source and do not run bindgen, expose generated `ic-cdk` call stubs, or pull
//! in broader DFINITY NNS crate dependencies.

use candid::Principal;

#[allow(clippy::all, unused_imports)]
mod generated {
    use candid::{CandidType, Deserialize, Principal};
    use serde::Serialize;

    include!("generated/nns_governance_types.rs");
}

pub use generated::{
    Account, AccountIdentifier, AddHotKey, BallotInfo, By, ClaimOrRefresh, ClaimOrRefreshResponse,
    Command, Configure, DisburseMaturity, DisburseMaturityResponse, DissolveState, Followees,
    GovernanceError, IncreaseDissolveDelay, KnownNeuronData, ListNeurons, ListNeuronsResponse,
    MakeProposalRequest, MakeProposalResponse, ManageNeuronCommandRequest, ManageNeuronRequest,
    ManageNeuronResponse, MaturityDisbursement, Motion, Neuron, NeuronId, NeuronIdOrSubaccount,
    NeuronInfo, NeuronStakeTransfer, NeuronSubaccount, Operation, ProposalActionRequest,
    ProposalId, RefreshVotingPower, RefreshVotingPowerResponse, RegisterVote, Result2,
    SetVisibility,
};

pub type PrincipalId = Principal;
pub type NeuronResult = Result2;

pub mod list_neurons {
    pub type NeuronSubaccount = super::NeuronSubaccount;
}

pub mod neuron {
    pub type DissolveState = super::DissolveState;
    pub type Followees = super::Followees;
}

pub mod manage_neuron {
    pub type AddHotKey = super::AddHotKey;
    pub type ClaimOrRefresh = super::ClaimOrRefresh;
    pub type Configure = super::Configure;
    pub type DisburseMaturity = super::DisburseMaturity;
    pub type IncreaseDissolveDelay = super::IncreaseDissolveDelay;
    pub type NeuronIdOrSubaccount = super::NeuronIdOrSubaccount;
    pub type RefreshVotingPower = super::RefreshVotingPower;
    pub type RegisterVote = super::RegisterVote;
    pub type SetVisibility = super::SetVisibility;

    pub mod claim_or_refresh {
        pub type By = super::super::By;
    }

    pub mod configure {
        pub type Operation = super::super::Operation;
    }
}

pub mod manage_neuron_response {
    pub type ClaimOrRefreshResponse = super::ClaimOrRefreshResponse;
    pub type Command = super::Command;
    pub type DisburseMaturityResponse = super::DisburseMaturityResponse;
    pub type MakeProposalResponse = super::MakeProposalResponse;
    pub type RefreshVotingPowerResponse = super::RefreshVotingPowerResponse;
}

pub mod governance_error {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
    #[repr(i32)]
    pub enum ErrorType {
        Unspecified = 0,
        Ok = 1,
        Unavailable = 2,
        NotAuthorized = 3,
        NotFound = 4,
        InvalidCommand = 5,
        RequiresNotDissolving = 6,
        RequiresDissolving = 7,
        RequiresDissolved = 8,
        HotKey = 9,
        ResourceExhausted = 10,
        PreconditionFailed = 11,
        External = 12,
        LedgerUpdateOngoing = 13,
        InsufficientFunds = 14,
        InvalidPrincipal = 15,
        InvalidProposal = 16,
        AlreadyJoinedCommunityFund = 17,
        NotInTheCommunityFund = 18,
        NeuronAlreadyVoted = 19,
    }
}

impl Default for NeuronId {
    fn default() -> Self {
        Self { id: 0 }
    }
}

impl Default for ProposalId {
    fn default() -> Self {
        Self { id: 0 }
    }
}

impl Default for BallotInfo {
    fn default() -> Self {
        Self {
            vote: 0,
            proposal_id: None,
        }
    }
}

impl Default for AccountIdentifier {
    fn default() -> Self {
        Self { hash: Vec::new() }
    }
}

impl Default for Account {
    fn default() -> Self {
        Self {
            owner: None,
            subaccount: None,
        }
    }
}

impl Default for MaturityDisbursement {
    fn default() -> Self {
        Self {
            account_identifier_to_disburse_to: None,
            timestamp_of_disbursement_seconds: None,
            amount_e8s: None,
            account_to_disburse_to: None,
            finalize_disbursement_timestamp_seconds: None,
        }
    }
}

impl Default for Followees {
    fn default() -> Self {
        Self {
            followees: Vec::new(),
        }
    }
}

impl Default for NeuronStakeTransfer {
    fn default() -> Self {
        Self {
            to_subaccount: Vec::new(),
            neuron_stake_e8s: 0,
            from: None,
            memo: 0,
            from_subaccount: Vec::new(),
            transfer_timestamp: 0,
            block_height: 0,
        }
    }
}

impl Default for KnownNeuronData {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: None,
        }
    }
}

impl Default for Neuron {
    fn default() -> Self {
        Self {
            id: None,
            staked_maturity_e8s_equivalent: None,
            controller: None,
            recent_ballots: Vec::new(),
            voting_power_refreshed_timestamp_seconds: None,
            kyc_verified: false,
            potential_voting_power: None,
            neuron_type: None,
            not_for_profit: false,
            maturity_e8s_equivalent: 0,
            deciding_voting_power: None,
            cached_neuron_stake_e8s: 0,
            created_timestamp_seconds: 0,
            auto_stake_maturity: None,
            aging_since_timestamp_seconds: 0,
            hot_keys: Vec::new(),
            account: Vec::new(),
            joined_community_fund_timestamp_seconds: None,
            eight_year_gang_bonus_base_e8s: None,
            maturity_disbursements_in_progress: None,
            dissolve_state: None,
            followees: Vec::new(),
            neuron_fees_e8s: 0,
            visibility: None,
            transfer: None,
            known_neuron_data: None,
            spawn_at_timestamp_seconds: None,
        }
    }
}

impl Default for GovernanceError {
    fn default() -> Self {
        Self {
            error_message: String::new(),
            error_type: 0,
        }
    }
}

impl Default for ListNeurons {
    fn default() -> Self {
        Self {
            page_size: None,
            include_public_neurons_in_full_neurons: None,
            neuron_ids: Vec::new(),
            page_number: None,
            include_empty_neurons_readable_by_caller: None,
            neuron_subaccounts: None,
            include_neurons_readable_by_caller: false,
        }
    }
}

impl Default for NeuronInfo {
    fn default() -> Self {
        Self {}
    }
}

impl Default for ListNeuronsResponse {
    fn default() -> Self {
        Self {
            neuron_infos: Vec::new(),
            full_neurons: Vec::new(),
            total_pages_available: None,
        }
    }
}

impl Default for DisburseMaturity {
    fn default() -> Self {
        Self {
            to_account_identifier: None,
            to_account: None,
            percentage_to_disburse: 0,
        }
    }
}

impl Default for RefreshVotingPower {
    fn default() -> Self {
        Self {}
    }
}

impl Default for ClaimOrRefresh {
    fn default() -> Self {
        Self { by: None }
    }
}

impl Default for AddHotKey {
    fn default() -> Self {
        Self { new_hot_key: None }
    }
}

impl Default for IncreaseDissolveDelay {
    fn default() -> Self {
        Self {
            additional_dissolve_delay_seconds: 0,
        }
    }
}

impl Default for SetVisibility {
    fn default() -> Self {
        Self { visibility: None }
    }
}

impl Default for Configure {
    fn default() -> Self {
        Self { operation: None }
    }
}

impl Default for RegisterVote {
    fn default() -> Self {
        Self {
            vote: 0,
            proposal: None,
        }
    }
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            motion_text: String::new(),
        }
    }
}

impl Default for MakeProposalRequest {
    fn default() -> Self {
        Self {
            url: String::new(),
            title: None,
            action: None,
            summary: String::new(),
        }
    }
}

impl Default for ManageNeuronRequest {
    fn default() -> Self {
        Self {
            id: None,
            command: None,
            neuron_id_or_subaccount: None,
        }
    }
}

impl Default for DisburseMaturityResponse {
    fn default() -> Self {
        Self {
            amount_disbursed_e8s: None,
        }
    }
}

impl Default for RefreshVotingPowerResponse {
    fn default() -> Self {
        Self {}
    }
}

impl Default for ClaimOrRefreshResponse {
    fn default() -> Self {
        Self {
            refreshed_neuron_id: None,
        }
    }
}

impl Default for MakeProposalResponse {
    fn default() -> Self {
        Self {
            message: None,
            proposal_id: None,
        }
    }
}

impl Default for ManageNeuronResponse {
    fn default() -> Self {
        Self { command: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use candid::{decode_one, encode_one};

    fn neuron_id(id: u64) -> NeuronId {
        NeuronId { id }
    }

    fn manage_request(command: ManageNeuronCommandRequest) -> ManageNeuronRequest {
        ManageNeuronRequest {
            id: None,
            neuron_id_or_subaccount: Some(NeuronIdOrSubaccount::NeuronId(neuron_id(42))),
            command: Some(command),
        }
    }

    #[test]
    fn list_neurons_round_trips_through_candid() {
        let request = ListNeurons {
            neuron_ids: vec![42],
            include_neurons_readable_by_caller: false,
            include_empty_neurons_readable_by_caller: Some(false),
            include_public_neurons_in_full_neurons: Some(true),
            page_number: None,
            page_size: None,
            neuron_subaccounts: None,
        };
        let decoded: ListNeurons = decode_one(&encode_one(&request).unwrap()).unwrap();
        assert_eq!(decoded.neuron_ids, vec![42]);

        let response = ListNeuronsResponse {
            neuron_infos: vec![(42, NeuronInfo {})],
            full_neurons: vec![Neuron {
                id: Some(neuron_id(42)),
                account: vec![1; 32],
                ..Default::default()
            }],
            ..Default::default()
        };
        let decoded: ListNeuronsResponse = decode_one(&encode_one(&response).unwrap()).unwrap();
        assert_eq!(
            decoded.full_neurons[0].id.as_ref().map(|id| id.id),
            Some(42)
        );
        assert_eq!(decoded.neuron_infos.len(), 1);
    }

    #[test]
    fn manage_neuron_claim_or_refresh_round_trips_through_candid() {
        let request = manage_request(ManageNeuronCommandRequest::ClaimOrRefresh(ClaimOrRefresh {
            by: Some(By::NeuronIdOrSubaccount {}),
        }));
        let decoded: ManageNeuronRequest = decode_one(&encode_one(&request).unwrap()).unwrap();
        assert!(matches!(
            decoded.command,
            Some(ManageNeuronCommandRequest::ClaimOrRefresh(_))
        ));

        let response = ManageNeuronResponse {
            command: Some(Command::ClaimOrRefresh(ClaimOrRefreshResponse {
                refreshed_neuron_id: Some(neuron_id(42)),
            })),
        };
        let decoded: ManageNeuronResponse = decode_one(&encode_one(&response).unwrap()).unwrap();
        assert!(matches!(decoded.command, Some(Command::ClaimOrRefresh(_))));
    }

    #[test]
    fn manage_neuron_error_round_trips_through_candid() {
        let response = ManageNeuronResponse {
            command: Some(Command::Error(GovernanceError {
                error_type: governance_error::ErrorType::PreconditionFailed as i32,
                error_message: "not allowed".to_string(),
            })),
        };

        let decoded: ManageNeuronResponse = decode_one(&encode_one(&response).unwrap()).unwrap();
        match decoded.command {
            Some(Command::Error(err)) => {
                assert_eq!(
                    err.error_type,
                    governance_error::ErrorType::PreconditionFailed as i32
                );
                assert_eq!(err.error_message, "not allowed");
            }
            other => panic!("expected manage_neuron error response, got {other:?}"),
        }
    }

    #[test]
    fn manage_neuron_refresh_voting_power_round_trips_through_candid() {
        let request = manage_request(ManageNeuronCommandRequest::RefreshVotingPower(
            RefreshVotingPower {},
        ));
        let decoded: ManageNeuronRequest = decode_one(&encode_one(&request).unwrap()).unwrap();
        assert!(matches!(
            decoded.command,
            Some(ManageNeuronCommandRequest::RefreshVotingPower(_))
        ));

        let response = ManageNeuronResponse {
            command: Some(Command::RefreshVotingPower(RefreshVotingPowerResponse {})),
        };
        let decoded: ManageNeuronResponse = decode_one(&encode_one(&response).unwrap()).unwrap();
        assert!(matches!(
            decoded.command,
            Some(Command::RefreshVotingPower(_))
        ));
    }

    #[test]
    fn manage_neuron_disburse_maturity_round_trips_through_candid() {
        let request = manage_request(ManageNeuronCommandRequest::DisburseMaturity(
            DisburseMaturity {
                percentage_to_disburse: 100,
                to_account: Some(Account {
                    owner: Some(Principal::anonymous()),
                    subaccount: Some(vec![7; 32]),
                }),
                to_account_identifier: None,
            },
        ));
        let decoded: ManageNeuronRequest = decode_one(&encode_one(&request).unwrap()).unwrap();
        assert!(matches!(
            decoded.command,
            Some(ManageNeuronCommandRequest::DisburseMaturity(_))
        ));

        let response = ManageNeuronResponse {
            command: Some(Command::DisburseMaturity(DisburseMaturityResponse {
                amount_disbursed_e8s: Some(1_000),
            })),
        };
        let decoded: ManageNeuronResponse = decode_one(&encode_one(&response).unwrap()).unwrap();
        assert!(matches!(
            decoded.command,
            Some(Command::DisburseMaturity(_))
        ));
    }

    #[test]
    fn get_full_neuron_result_round_trips_through_candid() {
        let response: Result<Neuron, GovernanceError> = Ok(Neuron {
            id: Some(neuron_id(42)),
            account: vec![1; 32],
            aging_since_timestamp_seconds: 1_234,
            maturity_disbursements_in_progress: Some(vec![MaturityDisbursement {
                amount_e8s: Some(55),
                ..Default::default()
            }]),
            ..Default::default()
        });
        let decoded: Result<Neuron, GovernanceError> =
            decode_one(&encode_one(&response).unwrap()).unwrap();
        assert_eq!(
            decoded.unwrap().maturity_disbursements_in_progress.unwrap()[0].amount_e8s,
            Some(55)
        );
    }
}
