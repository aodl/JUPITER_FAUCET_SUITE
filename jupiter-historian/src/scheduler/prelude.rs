use async_trait::async_trait;
use candid::{Nat, Principal};
use std::time::Duration;

use crate::clients::blackhole::BlackholeCanister;
use crate::clients::governance::NnsGovernanceCanister;
use crate::clients::index::{account_identifier_text_for_account, IcpIndexCanister};
use crate::clients::sns_root::{SnsCanisterSummary, SnsRootCanister};
use crate::clients::sns_wasm::SnsWasmCanister;
use crate::clients::xrc::XrcCanister;
use crate::clients::{
    BlackholeClient, ClientError, ExchangeRateClient, GovernanceClient, IndexClient,
    SnsRootClient, SnsWasmClient,
};
use crate::{
    logic, MAX_RECENT_INVALID_COMMITMENTS, MAX_RECENT_QUALIFYING_COMMITMENTS,
    MAX_RECENT_UNDER_THRESHOLD_COMMITMENTS,
};
use crate::state::{self, ActiveCyclesSweep, ActiveRouteSweep, ActiveSnsDiscovery, CanisterMeta, CanisterSource, CommitmentIndexFault, CyclesProbeResult, CyclesSampleSource, IndexedRouteKind, InvalidCommitment, RecentCommitment, RecentNeuronCommitment};

const PAGE_SIZE: u64 = 500;
const MAX_INITIAL_CYCLES_PROBE_QUEUE: usize = 256;
const MAIN_TICK_LEASE_SECONDS: u64 = 15 * 60;
const ICP_XDR_RATE_CACHE_TTL_SECONDS: u64 = 24 * 60 * 60;
