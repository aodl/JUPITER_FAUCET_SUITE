import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { IDL } from '@icp-sdk/core/candid';
import { Principal } from '@icp-sdk/core/principal';
import { idlFactory } from '../declarations/icp_index/icp_index.did.js';
import { accountIdentifierHex } from '../src/data/dashboard-transforms.js';
import { loadIncomingIcpTransfersFromIndex, loadRecentRouteTransfersFromIndex } from '../src/data/index-transactions.js';

const owner = { owner: Principal.anonymous(), subaccount: [] };
const recipient = { owner: Principal.managementCanister(), subaccount: [Array(32).fill(89)] };
const spender = { owner: Principal.fromUint8Array(Uint8Array.of(88)), subaccount: [] };
const ownerId = accountIdentifierHex(owner);
const recipientId = accountIdentifierHex(recipient);
const spenderId = accountIdentifierHex(spender);

function realDelegatedTransfer() {
  const hex = readFileSync(new URL('../../../../crates/ic-clients/tests/fixtures/icp-index-delegated-transfer-response.hex', import.meta.url), 'utf8').replace(/\s+/g, '');
  const method = idlFactory({ IDL })._fields.find(([name]) => name === 'get_account_identifier_transactions')[1];
  const [response] = IDL.decode(method.retTypes, Uint8Array.from(Buffer.from(hex, 'hex')));
  assert.ok(response.Ok);
  const transaction = response.Ok.transactions.find((entry) => entry.id === 4n);
  assert.ok(transaction);
  return transaction;
}

function recipientIndex(transaction) {
  return {
    async get_account_identifier_transactions(args) {
      assert.equal(args.account_identifier, recipientId);
      assert.deepEqual(args.start, []);
      // The frozen reply is the owner's history. The receiver's history contains
      // this same real Transfer row, without the owner's separate Approve row.
      return { Ok: { balance: 20_000_000n, oldest_tx_id: [transaction.id], transactions: [transaction] } };
    },
  };
}

test('browser Index declaration decodes the frozen real delegated Transfer with its optional spender', () => {
  const transaction = realDelegatedTransfer();
  assert.deepEqual(transaction.transaction.operation.Transfer, {
    from: ownerId, to: recipientId, spender: [spenderId], amount: { e8s: 20_000_000n }, fee: { e8s: 10_000n },
  });
  assert.notEqual(ownerId, spenderId);
});

test('real delegated route payment retains source ownership and its transferred amount', async () => {
  const transaction = realDelegatedTransfer();
  const result = await loadRecentRouteTransfersFromIndex({
    index: recipientIndex(transaction), outputSourceAccount: owner, routeAccount: recipient,
  });
  assert.equal(result.items.length, 1);
  assert.equal(result.items[0].tx_id, 4n);
  assert.equal(result.items[0].amount_e8s, 20_000_000n);
  const wrongSource = await loadRecentRouteTransfersFromIndex({
    index: recipientIndex(transaction), outputSourceAccount: spender, routeAccount: recipient,
  });
  assert.deepEqual(wrongSource.items, []);
});

test('real delegated incoming payment attributes the owner rather than the spender', async () => {
  const transaction = realDelegatedTransfer();
  for (const [source, matches] of [[ownerId, true], [spenderId, false]]) {
    const result = await loadIncomingIcpTransfersFromIndex({
      index: recipientIndex(transaction), account: recipient, sourceAccountIdentifiers: [source],
      memoText: 'real-index-transfer-from',
    });
    assert.equal(result.items.length, 1);
    assert.equal(result.items[0].from_account_identifier, ownerId);
    assert.equal(result.items[0].amount_e8s, 20_000_000n);
    assert.equal(result.items[0].is_matching_source, matches);
    assert.equal(result.items[0].is_matching_memo, true);
    assert.equal(result.truncated, false);
  }
});
