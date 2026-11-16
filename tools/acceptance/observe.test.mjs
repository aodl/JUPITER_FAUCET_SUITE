import assert from 'node:assert/strict';
import test from 'node:test';
import { Principal } from '@icp-sdk/core/principal';
import { validateWire } from './observe.mjs';
const ledger = 'ryjl3-tyaaa-aaaaa-aaaba-cai';
const other = 'rrkah-fqaaa-aaaaa-aaaaq-cai';
const bytes = id => Principal.fromText(id).toUint8Array();
const text = x => new TextEncoder().encode(x);
const request = () => ({url:`https://icp-api.io/api/v3/canister/${ledger}/query`,
  c:{request_type:'query',sender:bytes('2vxsx-fae'),canister_id:bytes(ledger),method_name:'icrc1_fee'},
  active:{target:ledger,method:'icrc1_fee'},targets:new Set([ledger]),ledgers:[ledger],caller:'2vxsx-fae',host:'https://icp-api.io'});
test('only anonymous allowlisted query endpoints; no discovery, mutation or alternate target', () => {
  validateWire(request());
  for (const patch of [
    r=>{r.c.request_type='call';},r=>{r.c.method_name='icrc1_transfer';},
    r=>{r.c.method_name='__get_candid_interface_tmp_hack';},
    r=>{r.c.sender=bytes(other);},r=>{r.c.canister_id=bytes(other);},
    r=>{r.url=`https://icp-api.io/api/v3/canister/${other}/query`;},
    r=>{r.url=`https://example.com/api/v3/canister/${ledger}/query`;},
  ]) {const r=request();patch(r);assert.throws(()=>validateWire(r));}
});
test('certified paths and separately classified SDK verification support', () => {
  const r=request();r.url=r.url.replace('/query','/read_state');r.c.request_type='read_state';
  r.c.paths=[[text('canister'),bytes(ledger),text('module_hash')]];
  r.active.method='read_state';validateWire(r);
  r.c.paths=[[text('canister'),bytes(other),text('controllers')]];
  assert.throws(()=>validateWire(r));
  r.c.paths=[[text('canister'),bytes(ledger),text('metadata'),text('candid:service')]];
  assert.throws(()=>validateWire(r));
  r.c.paths=[[text('subnet'),bytes(other),text('node')]];
  assert.throws(()=>validateWire(r));
  r.active.method='icrc1_fee';validateWire(r);
  r.c.paths=[[text('time')]];assert.throws(()=>validateWire(r));
});
