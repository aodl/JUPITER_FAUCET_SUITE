//! Minimal NNS Governance Candid wire types used by Jupiter.
//!
//! These local types intentionally replace broad DFINITY NNS/governance crate
//! dependencies such as `ic-base-types`, `ic-nns-common`, and
//! `ic-nns-governance-api`. They are Candid wire DTOs, not semantic forks of NNS
//! governance behavior. Field names, optionality, numeric representations, blob
//! representations, and variant names must preserve upstream wire
//! compatibility. Changes here should be validated against the pinned NNS
//! Governance DID and the existing NNS, PocketIC, and e2e test gates.

use std::collections::HashMap;

use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};

pub type PrincipalId = Principal;

#[derive(
    CandidType,
    Deserialize,
    Serialize,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Debug,
    Default,
)]
pub struct NeuronId {
    pub id: u64,
}

#[derive(
    CandidType,
    Deserialize,
    Serialize,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Debug,
    Default,
)]
pub struct ProposalId {
    pub id: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
pub struct Empty {}

#[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
pub struct GovernanceError {
    pub error_type: i32,
    pub error_message: String,
}

pub mod governance_error {
    use super::*;

    #[derive(
        CandidType, Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord,
    )]
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

#[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
pub struct Account {
    pub owner: Option<PrincipalId>,
    pub subaccount: Option<Vec<u8>>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
pub struct AccountIdentifier {
    pub hash: Vec<u8>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
pub struct MaturityDisbursement {
    pub amount_e8s: Option<u64>,
    pub timestamp_of_disbursement_seconds: Option<u64>,
    pub finalize_disbursement_timestamp_seconds: Option<u64>,
    pub account_to_disburse_to: Option<Account>,
    pub account_identifier_to_disburse_to: Option<AccountIdentifier>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
pub struct Neuron {
    pub id: Option<NeuronId>,
    pub account: Vec<u8>,
    pub controller: Option<PrincipalId>,
    pub hot_keys: Vec<PrincipalId>,
    pub cached_neuron_stake_e8s: u64,
    pub neuron_fees_e8s: u64,
    pub created_timestamp_seconds: u64,
    pub aging_since_timestamp_seconds: u64,
    pub spawn_at_timestamp_seconds: Option<u64>,
    pub followees: HashMap<i32, neuron::Followees>,
    pub recent_ballots: Vec<BallotInfo>,
    pub kyc_verified: bool,
    pub transfer: Option<NeuronStakeTransfer>,
    pub maturity_e8s_equivalent: u64,
    pub staked_maturity_e8s_equivalent: Option<u64>,
    pub auto_stake_maturity: Option<bool>,
    pub not_for_profit: bool,
    pub joined_community_fund_timestamp_seconds: Option<u64>,
    pub known_neuron_data: Option<KnownNeuronData>,
    pub neuron_type: Option<i32>,
    pub visibility: Option<i32>,
    pub voting_power_refreshed_timestamp_seconds: Option<u64>,
    pub dissolve_state: Option<neuron::DissolveState>,
    pub deciding_voting_power: Option<u64>,
    pub potential_voting_power: Option<u64>,
    pub eight_year_gang_bonus_base_e8s: Option<u64>,
    pub maturity_disbursements_in_progress: Option<Vec<MaturityDisbursement>>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
pub struct BallotInfo {}

#[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
pub struct KnownNeuronData {
    pub name: String,
    pub description: Option<String>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
pub struct NeuronStakeTransfer {
    pub transfer_timestamp: u64,
    pub from: Option<PrincipalId>,
    pub from_subaccount: Vec<u8>,
    pub to_subaccount: Vec<u8>,
    pub neuron_stake_e8s: u64,
    pub block_height: u64,
    pub memo: u64,
}

pub mod neuron {
    use super::*;

    #[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
    pub struct Followees {
        pub followees: Vec<NeuronId>,
    }

    #[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug)]
    pub enum DissolveState {
        WhenDissolvedTimestampSeconds(u64),
        DissolveDelaySeconds(u64),
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
pub struct ListNeurons {
    pub neuron_ids: Vec<u64>,
    pub include_neurons_readable_by_caller: bool,
    pub include_empty_neurons_readable_by_caller: Option<bool>,
    pub include_public_neurons_in_full_neurons: Option<bool>,
    pub page_number: Option<u64>,
    pub page_size: Option<u64>,
    pub neuron_subaccounts: Option<Vec<list_neurons::NeuronSubaccount>>,
}

pub mod list_neurons {
    use super::*;

    #[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
    pub struct NeuronSubaccount {
        pub subaccount: Vec<u8>,
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
pub struct ListNeuronsResponse {
    pub full_neurons: Vec<Neuron>,
    pub total_pages_available: Option<u64>,
}

pub mod manage_neuron {
    use super::*;

    #[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
    pub struct IncreaseDissolveDelay {
        pub additional_dissolve_delay_seconds: u32,
    }

    #[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
    pub struct AddHotKey {
        pub new_hot_key: Option<PrincipalId>,
    }

    #[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
    pub struct SetVisibility {
        pub visibility: Option<i32>,
    }

    #[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
    pub struct Configure {
        pub operation: Option<configure::Operation>,
    }

    pub mod configure {
        use super::*;

        #[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug)]
        pub enum Operation {
            IncreaseDissolveDelay(IncreaseDissolveDelay),
            AddHotKey(AddHotKey),
            SetVisibility(SetVisibility),
        }
    }

    #[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
    pub struct RegisterVote {
        pub proposal: Option<ProposalId>,
        pub vote: i32,
    }

    #[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
    pub struct ClaimOrRefresh {
        pub by: Option<claim_or_refresh::By>,
    }

    pub mod claim_or_refresh {
        use super::*;

        #[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug)]
        pub enum By {
            Memo(u64),
            NeuronIdOrSubaccount(Empty),
        }
    }

    #[derive(CandidType, Deserialize, Serialize, Clone, Copy, PartialEq, Debug, Default)]
    pub struct RefreshVotingPower {}

    #[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
    pub struct DisburseMaturity {
        pub percentage_to_disburse: u32,
        pub to_account: Option<Account>,
        pub to_account_identifier: Option<AccountIdentifier>,
    }

    #[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug)]
    pub enum NeuronIdOrSubaccount {
        Subaccount(Vec<u8>),
        NeuronId(NeuronId),
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
pub struct Motion {
    pub motion_text: String,
}

#[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug)]
pub enum ProposalActionRequest {
    Motion(Motion),
}

#[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
pub struct MakeProposalRequest {
    pub title: Option<String>,
    pub summary: String,
    pub url: String,
    pub action: Option<ProposalActionRequest>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
pub struct ManageNeuronRequest {
    pub id: Option<NeuronId>,
    pub neuron_id_or_subaccount: Option<manage_neuron::NeuronIdOrSubaccount>,
    pub command: Option<ManageNeuronCommandRequest>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug)]
pub enum ManageNeuronCommandRequest {
    Configure(manage_neuron::Configure),
    MakeProposal(Box<MakeProposalRequest>),
    RegisterVote(manage_neuron::RegisterVote),
    ClaimOrRefresh(manage_neuron::ClaimOrRefresh),
    RefreshVotingPower(manage_neuron::RefreshVotingPower),
    DisburseMaturity(manage_neuron::DisburseMaturity),
}

#[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
pub struct ManageNeuronResponse {
    pub command: Option<manage_neuron_response::Command>,
}

pub mod manage_neuron_response {
    use super::*;

    #[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
    pub struct ConfigureResponse {}

    #[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
    pub struct MakeProposalResponse {
        pub proposal_id: Option<ProposalId>,
        pub message: Option<String>,
    }

    #[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
    pub struct RegisterVoteResponse {}

    #[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug, Default)]
    pub struct ClaimOrRefreshResponse {
        pub refreshed_neuron_id: Option<NeuronId>,
    }

    #[derive(CandidType, Deserialize, Serialize, Clone, Copy, PartialEq, Debug, Default)]
    pub struct RefreshVotingPowerResponse {}

    #[derive(CandidType, Deserialize, Serialize, Clone, Copy, PartialEq, Debug, Default)]
    pub struct DisburseMaturityResponse {
        pub amount_disbursed_e8s: Option<u64>,
    }

    #[derive(CandidType, Deserialize, Serialize, Clone, PartialEq, Debug)]
    pub enum Command {
        Error(GovernanceError),
        Configure(ConfigureResponse),
        MakeProposal(MakeProposalResponse),
        RegisterVote(RegisterVoteResponse),
        ClaimOrRefresh(ClaimOrRefreshResponse),
        RefreshVotingPower(RefreshVotingPowerResponse),
        DisburseMaturity(DisburseMaturityResponse),
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
            neuron_id_or_subaccount: Some(manage_neuron::NeuronIdOrSubaccount::NeuronId(
                neuron_id(42),
            )),
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
    }

    #[test]
    fn manage_neuron_claim_or_refresh_round_trips_through_candid() {
        let request = manage_request(ManageNeuronCommandRequest::ClaimOrRefresh(
            manage_neuron::ClaimOrRefresh {
                by: Some(manage_neuron::claim_or_refresh::By::NeuronIdOrSubaccount(
                    Empty {},
                )),
            },
        ));
        let decoded: ManageNeuronRequest = decode_one(&encode_one(&request).unwrap()).unwrap();
        assert!(matches!(
            decoded.command,
            Some(ManageNeuronCommandRequest::ClaimOrRefresh(_))
        ));

        let response = ManageNeuronResponse {
            command: Some(manage_neuron_response::Command::ClaimOrRefresh(
                manage_neuron_response::ClaimOrRefreshResponse {
                    refreshed_neuron_id: Some(neuron_id(42)),
                },
            )),
        };
        let decoded: ManageNeuronResponse = decode_one(&encode_one(&response).unwrap()).unwrap();
        assert!(matches!(
            decoded.command,
            Some(manage_neuron_response::Command::ClaimOrRefresh(_))
        ));
    }

    #[test]
    fn manage_neuron_error_round_trips_through_candid() {
        let response = ManageNeuronResponse {
            command: Some(manage_neuron_response::Command::Error(GovernanceError {
                error_type: governance_error::ErrorType::PreconditionFailed as i32,
                error_message: "not allowed".to_string(),
            })),
        };

        let decoded: ManageNeuronResponse = decode_one(&encode_one(&response).unwrap()).unwrap();
        match decoded.command {
            Some(manage_neuron_response::Command::Error(err)) => {
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
            manage_neuron::RefreshVotingPower {},
        ));
        let decoded: ManageNeuronRequest = decode_one(&encode_one(&request).unwrap()).unwrap();
        assert!(matches!(
            decoded.command,
            Some(ManageNeuronCommandRequest::RefreshVotingPower(_))
        ));

        let response = ManageNeuronResponse {
            command: Some(manage_neuron_response::Command::RefreshVotingPower(
                manage_neuron_response::RefreshVotingPowerResponse {},
            )),
        };
        let decoded: ManageNeuronResponse = decode_one(&encode_one(&response).unwrap()).unwrap();
        assert!(matches!(
            decoded.command,
            Some(manage_neuron_response::Command::RefreshVotingPower(_))
        ));
    }

    #[test]
    fn manage_neuron_disburse_maturity_round_trips_through_candid() {
        let request = manage_request(ManageNeuronCommandRequest::DisburseMaturity(
            manage_neuron::DisburseMaturity {
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
            command: Some(manage_neuron_response::Command::DisburseMaturity(
                manage_neuron_response::DisburseMaturityResponse {
                    amount_disbursed_e8s: Some(1_000),
                },
            )),
        };
        let decoded: ManageNeuronResponse = decode_one(&encode_one(&response).unwrap()).unwrap();
        assert!(matches!(
            decoded.command,
            Some(manage_neuron_response::Command::DisburseMaturity(_))
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
