import { createActor as createLedgerActor } from '../../declarations/icp_ledger/index.js';
import { createActor as createIndexActor } from '../../declarations/icp_index/index.js';
import { createActor as createHistorianActor } from '../../declarations/jupiter_historian/index.js';
import { createHistorianClient, isMethodMissingError, normalizeError } from '../app/agent.js';
import { loadRecentRouteTransfersFromIndex } from './index-transactions.js';
import {
  RECENT_COMMITMENT_LIMIT,
  REGISTERED_SUMMARY_PAGE_SIZE,
  accountIdentifierBytes,
  buildRegisteredCanisterSummariesRequest,
  dquorumStakingAccount,
  principalToText,
  readOptional,
} from './dashboard-transforms.js';

export async function loadMemoRegisteredCanisterSummaryPage({
  historianCanisterId,
  host,
  local = false,
  agent = null,
  historianActor = null,
  historianActorFactory = createHistorianActor,
  page = 0,
  pageSize = REGISTERED_SUMMARY_PAGE_SIZE,
} = {}) {
  const { historian } = await createHistorianClient({
    historianCanisterId,
    host,
    local,
    agent,
    historianActor,
    historianActorFactory,
  });
  return historian.list_memo_registered_canister_summaries(
    buildRegisteredCanisterSummariesRequest({ page, pageSize }),
  );
}

export async function loadCanisterModuleHashes({
  historianCanisterId,
  host,
  local = false,
  agent = null,
  historianActor = null,
  historianActorFactory = createHistorianActor,
} = {}) {
  const { historian } = await createHistorianClient({
    historianCanisterId,
    host,
    local,
    agent,
    historianActor,
    historianActorFactory,
  });
  return historian.get_canister_module_hashes();
}

async function loadRouteTransferTables({ status, agent, indexActorFactory }) {
  const indexCanisterId = principalToText(status?.index_canister_id);
  const outputSourceAccount = readOptional(status?.output_source_account);
  const outputAccount = readOptional(status?.output_account);
  const rewardsAccount = readOptional(status?.rewards_account);
  const dquorumAccount = dquorumStakingAccount();

  if (!indexCanisterId || !outputSourceAccount || !outputAccount || !rewardsAccount) {
    return {
      outputTransfersResult: { status: 'fulfilled', value: { items: [] } },
      rewardsTransfersResult: { status: 'fulfilled', value: { items: [] } },
      dquorumTransfersResult: { status: 'fulfilled', value: { items: [] } },
    };
  }

  const index = indexActorFactory(indexCanisterId, { agent });
  const [outputTransfersResult, rewardsTransfersResult, dquorumTransfersResult] = await Promise.allSettled([
    loadRecentRouteTransfersFromIndex({
      index,
      outputSourceAccount,
      routeAccount: outputAccount,
    }),
    loadRecentRouteTransfersFromIndex({
      index,
      outputSourceAccount,
      routeAccount: rewardsAccount,
    }),
    loadRecentRouteTransfersFromIndex({
      index,
      outputSourceAccount,
      routeAccount: dquorumAccount,
    }),
  ]);

  return { outputTransfersResult, rewardsTransfersResult, dquorumTransfersResult };
}

export async function loadDashboardData({
  historianCanisterId,
  host,
  local = false,
  agent = null,
  historianActor = null,
  historianActorFactory = createHistorianActor,
  ledgerActorFactory = createLedgerActor,
  indexActorFactory = createIndexActor,
  registeredPage = 0,
  registeredPageSize = REGISTERED_SUMMARY_PAGE_SIZE,
} = {}) {
  let resolvedAgent;
  let historian;
  try {
    ({ agent: resolvedAgent, historian } = await createHistorianClient({
      historianCanisterId,
      host,
      local,
      agent,
      historianActor,
      historianActorFactory,
    }));
  } catch (error) {
    const reason = normalizeError(error);
    return {
      counts: null,
      status: null,
      registered: null,
      recent: null,
      outputTransfers: null,
      rewardsTransfers: null,
      dquorumTransfers: null,
      stakeE8s: null,
      hasAnyFailure: true,
      errors: {
        counts: reason,
        status: reason,
        registered: reason,
        recent: reason,
        outputTransfers: reason,
        rewardsTransfers: reason,
        dquorumTransfers: reason,
        stake: 'Stake unavailable',
      },
      historianAllRejected: true,
      historianLikelyOutdated: isMethodMissingError(error),
    };
  }

  const [countsResult, statusResult, registeredResult, recentResult] = await Promise.allSettled([
    historian.get_public_counts(),
    historian.get_public_status(),
    historian.list_memo_registered_canister_summaries(
      buildRegisteredCanisterSummariesRequest({
        page: registeredPage,
        pageSize: registeredPageSize,
      }),
    ),
    historian.list_recent_commitments({
      limit: [RECENT_COMMITMENT_LIMIT],
      qualifying_only: [false],
    }),
  ]);

  let stakeResult = { status: 'rejected', reason: new Error('Stake unavailable') };
  let outputTransfersResult = { status: 'fulfilled', value: { items: [] } };
  let rewardsTransfersResult = { status: 'fulfilled', value: { items: [] } };
  let dquorumTransfersResult = { status: 'fulfilled', value: { items: [] } };

  if (statusResult.status === 'fulfilled') {
    let ledger;
    try {
      ledger = ledgerActorFactory(statusResult.value.ledger_canister_id.toText(), {
        agent: resolvedAgent,
      });
    } catch (error) {
      ledger = null;
      stakeResult = { status: 'rejected', reason: error };
    }
    if (ledger) {
      const stakingAccount = statusResult.value.staking_account;
      const stakeAccountId = accountIdentifierBytes(stakingAccount);
      stakeResult = await ledger
        .account_balance({ account: stakeAccountId })
        .then((value) => ({ status: 'fulfilled', value: value.e8s }))
        .catch(async (nativeReason) => {
          try {
            const fallbackValue = await ledger.icrc1_balance_of(stakingAccount);
            return { status: 'fulfilled', value: fallbackValue };
          } catch (icrcReason) {
            return {
              status: 'rejected',
              reason: new Error(`Stake unavailable via native ledger account_balance (${normalizeError(nativeReason)}) and icrc1_balance_of (${normalizeError(icrcReason)})`),
            };
          }
        });
    }

    try {
      ({ outputTransfersResult, rewardsTransfersResult, dquorumTransfersResult } = await loadRouteTransferTables({
        status: statusResult.value,
        agent: resolvedAgent,
        indexActorFactory,
      }));
    } catch (error) {
      outputTransfersResult = { status: 'rejected', reason: error };
      rewardsTransfersResult = { status: 'rejected', reason: error };
      dquorumTransfersResult = { status: 'rejected', reason: error };
    }
  }

  const countsValue = countsResult.status === 'fulfilled' ? countsResult.value : null;

  const errors = {
    counts: countsResult.status === 'rejected' ? normalizeError(countsResult.reason) : null,
    status: statusResult.status === 'rejected' ? normalizeError(statusResult.reason) : null,
    registered: registeredResult.status === 'rejected' ? normalizeError(registeredResult.reason) : null,
    recent: recentResult.status === 'rejected' ? normalizeError(recentResult.reason) : null,
    outputTransfers: outputTransfersResult.status === 'rejected' ? normalizeError(outputTransfersResult.reason) : null,
    rewardsTransfers: rewardsTransfersResult.status === 'rejected' ? normalizeError(rewardsTransfersResult.reason) : null,
    dquorumTransfers: dquorumTransfersResult.status === 'rejected' ? normalizeError(dquorumTransfersResult.reason) : null,
    stake: stakeResult.status === 'rejected' ? normalizeError(stakeResult.reason) : null,
  };

  const historianFailures = [countsResult, statusResult, registeredResult, recentResult].filter((result) => result.status === 'rejected');
  const historianAllRejected = historianFailures.length === 4;
  const historianLikelyOutdated = historianAllRejected && historianFailures.every((result) => isMethodMissingError(result.reason));

  return {
    counts: countsValue,
    status: statusResult.status === 'fulfilled' ? statusResult.value : null,
    registered: registeredResult.status === 'fulfilled' ? registeredResult.value : null,
    recent: recentResult.status === 'fulfilled' ? recentResult.value : null,
    outputTransfers: outputTransfersResult.status === 'fulfilled' ? outputTransfersResult.value : null,
    rewardsTransfers: rewardsTransfersResult.status === 'fulfilled' ? rewardsTransfersResult.value : null,
    dquorumTransfers: dquorumTransfersResult.status === 'fulfilled' ? dquorumTransfersResult.value : null,
    stakeE8s: stakeResult.status === 'fulfilled' ? stakeResult.value : null,
    hasAnyFailure:
      countsResult.status === 'rejected' ||
      statusResult.status === 'rejected' ||
      registeredResult.status === 'rejected' ||
      recentResult.status === 'rejected' ||
      stakeResult.status === 'rejected',
    errors,
    historianAllRejected,
    historianLikelyOutdated,
  };
}
