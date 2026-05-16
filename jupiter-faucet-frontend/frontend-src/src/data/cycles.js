import { IDL } from '@icp-sdk/core/candid';
import { MANAGEMENT_CANISTER_ID } from '@icp-sdk/core/agent';

const FetchCanisterLogsArgs = IDL.Record({
  canister_id: IDL.Principal,
});
const CanisterLogRecord = IDL.Record({
  idx: IDL.Nat64,
  timestamp_nanos: IDL.Nat64,
  content: IDL.Vec(IDL.Nat8),
});
const FetchCanisterLogsResult = IDL.Record({
  canister_log_records: IDL.Vec(CanisterLogRecord),
});
const LOG_TEXT_DECODER = new TextDecoder('utf-8', { fatal: false });

function queryReplyArg(response) {
  return response?.reply?.arg || response?.arg || response?.certificate?.reply?.arg || null;
}

function normalizeCanisterLogRecord(record) {
  const content = Uint8Array.from(record?.content || []);
  return {
    idx: record?.idx,
    timestamp_nanos: record?.timestamp_nanos,
    content,
    text: LOG_TEXT_DECODER.decode(content),
  };
}

export async function loadCanisterLogs({ agent, canisterId } = {}) {
  if (!agent || typeof agent.query !== 'function') {
    throw new Error('HTTP agent is unavailable');
  }
  if (!canisterId) {
    throw new Error('A canister ID is required');
  }

  const arg = IDL.encode([FetchCanisterLogsArgs], [{ canister_id: canisterId }]);
  const response = await agent.query(MANAGEMENT_CANISTER_ID, {
    methodName: 'fetch_canister_logs',
    arg,
    effectiveCanisterId: canisterId,
  });

  if (response?.status === 'rejected') {
    throw new Error(response.reject_message || response.reject_code || 'Canister logs query was rejected');
  }
  const replyArg = queryReplyArg(response);
  if (!replyArg) {
    throw new Error('Canister logs query returned an unexpected response');
  }
  const [result] = IDL.decode([FetchCanisterLogsResult], replyArg);
  return {
    items: (result?.canister_log_records || []).map(normalizeCanisterLogRecord),
  };
}
