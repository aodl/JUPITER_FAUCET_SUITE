// Anonymous bounded transport only. Invoked by observe.py; never loads identities.
import fs from 'node:fs';
import { createHash } from 'node:crypto';
import { HttpAgent, AnonymousIdentity, Certificate, Cbor, lookupResultToBuffer } from '@icp-sdk/core/agent';
import { IDL } from '@icp-sdk/core/candid';
import { Principal } from '@icp-sdk/core/principal';
export function validateWire({url, c, active, targets, ledgers, caller, host, governance, rewards}) {
  const u = new URL(url);
  const m = u.pathname.match(/^\/api\/v[23]\/canister\/([^/]+)\/(query|read_state)$/);
  if (u.origin !== host || !m || !targets.has(m[1]) || m[1] !== active.target ||
      Principal.fromUint8Array(c.sender).toText() !== caller) throw Error('transport isolation');
  if (!['query','read_state'].includes(c.request_type) || m[2] !== c.request_type) throw Error('writes forbidden');
  if (c.request_type === 'query') {
    const allowed = ledgers.includes(active.target) ? ['icrc1_fee','icrc1_total_supply','icrc1_balance_of'] :
      active.target === governance ? ['get_running_sns_version','get_nervous_system_parameters','get_upgrade_journal'] :
      active.target === rewards ? ['get_programme_status'] : [];
    if (!allowed.includes(c.method_name) || c.method_name !== active.method ||
        Principal.fromUint8Array(c.canister_id).toText() !== active.target) throw Error('method denied');
  }
  if (c.request_type === 'read_state') {
    for (const path of c.paths) {
      const label = new TextDecoder().decode(path[0]);
      if (!['canister','subnet'].includes(label)) throw Error('unexpected certificate path');
      if (label === 'canister' && (Principal.fromUint8Array(path[1]).toText() !== active.target ||
          !['module_hash','controllers'].includes(new TextDecoder().decode(path[2])))) throw Error('certificate path denied');
      if (label === 'subnet' && active.method === 'read_state') throw Error('unexpected support read');
    }
  }
}

export function publicQueries(roles) {
const Version = IDL.Record(Object.fromEntries(['root','governance','ledger','index','swap','archive'].map(r=>[r+'_wasm_hash',IDL.Vec(IDL.Nat8)])));
const Empty = IDL.Record({});
return [
  [roles.sns_governance,'get_running_sns_version',[Empty],[{}],[IDL.Record({deployed_version:IDL.Opt(Version),pending_version:IDL.Opt(IDL.Record({target_version:IDL.Opt(Version),proposal_id:IDL.Nat64,checking_upgrade_lock:IDL.Nat64,mark_failed_at_seconds:IDL.Nat64}))})]],
  [roles.sns_governance,'get_nervous_system_parameters',[IDL.Null],[null],[IDL.Record({automatically_advance_target_version:IDL.Opt(IDL.Bool)})]],
  [roles.sns_governance,'get_upgrade_journal',[IDL.Record({limit:IDL.Opt(IDL.Nat64),offset:IDL.Opt(IDL.Nat64)})],[{limit:[20n],offset:[]}],[IDL.Record({target_version:IDL.Opt(Version),deployed_version:IDL.Opt(Version),upgrade_journal_entry_count:IDL.Opt(IDL.Nat64)})]],
  [roles.jupiter_sns_rewards,'get_programme_status',[],[],[IDL.Record({config:IDL.Opt(IDL.Record({version:IDL.Nat64,patron_enabled:IDL.Bool,launch_timestamp_seconds:IDL.Opt(IDL.Nat64)})),last_error:IDL.Opt(IDL.Text),verified_e8s:IDL.Nat,assumed_e8s:IDL.Nat,failed_items:IDL.Nat64})]],
];
}

export async function collect(profilePath, output) {
const p = JSON.parse(fs.readFileSync(profilePath));
if (p.environment !== 'acceptance' || p.network !== 'ic' || Object.keys(p.roles).length !== 12) throw Error('acceptance only');
const canonical = JSON.parse(fs.readFileSync(new URL('../../deploy/acceptance/profile.json', import.meta.url)));
if (JSON.stringify(p.roles) !== JSON.stringify(canonical.roles)) throw Error('assigned acceptance targets only');
const accounts = JSON.parse(fs.readFileSync(new URL('../../deploy/acceptance/derived-accounts.json', import.meta.url)));
const icp = 'ryjl3-tyaaa-aaaaa-aaaba-cai';
const ledgers = [icp, p.roles.sns_ledger];
const targets = new Set([...Object.values(p.roles), icp]);
const result = { kind: 'anonymous-sdk-collection', caller: '2vxsx-fae', network: 'ic', host: 'https://icp-api.io',
  started: Date.now(), observations: [], traffic: [], complete: false };
const hex = b => Buffer.from(b).toString('hex');
const clean = x => typeof x === 'bigint' ? x.toString() : x?._isPrincipal ? x.toText() :
  ArrayBuffer.isView(x) ? {hex: hex(x)} : Array.isArray(x) ? x.map(clean) :
  x && typeof x === 'object' ? Object.fromEntries(Object.entries(x).map(([k,v]) => [k,clean(v)])) : x;
const persist = () => {
  fs.writeFileSync(output+'.next', JSON.stringify(clean(result))+'\n');
  fs.renameSync(output+'.next', output+'.partial');
};
const seen = new Set(); let active = null; let totalBytes = 0;
const transport = async (url, options) => {
  if (!active || Date.now() > active.until || Date.now() - result.started > 180000 || result.traffic.length >= 128) throw Error('read budget exhausted');
  const u = new URL(url);
  const c = Cbor.decode(options.body).content;
  validateWire({url, c, active, targets, ledgers, caller:result.caller, host:result.host, governance:p.roles.sns_governance, rewards:p.roles.jupiter_sns_rewards});
  const key = JSON.stringify([u.pathname,c.method_name, c.arg ? hex(c.arg) : null, clean(c.paths)]);
  if (seen.has(key)) throw Error('retry blocked'); seen.add(key);
  const entry = {target: active.target, method: c.method_name || 'read_state',
    support: c.request_type === 'read_state' && active.method !== 'read_state', request_sha256: createHash('sha256').update(options.body).digest('hex'),
    arguments_hex: c.arg ? hex(c.arg) : null, paths: clean(c.paths), started: Date.now()};
  result.traffic.push(entry);
  persist(); // Preserve attempted wire budget even if a later read times out.
  const response = await fetch(url, {...options, redirect: 'error', signal: AbortSignal.timeout(Math.max(1, Math.min(30000, active.until-Date.now())))});
  const reader = response.body.getReader(); const chunks = []; let size = 0;
  while (true) {
    const {done, value} = await reader.read(); if (done) break;
    size += value.length; totalBytes += value.length;
    if (size > 2 * 1024 * 1024 || totalBytes > 16 * 1024 * 1024) {
      await reader.cancel(); throw Error('response budget exceeded');
    }
    chunks.push(value);
  }
  const bytes = Buffer.concat(chunks);
  entry.http_status = response.status;
  entry.response_hex = hex(bytes); // Bounded evidence, no credentials or private user data.
  persist();
  return new Response(bytes, {status: response.status, headers: response.headers});
};
const agent = HttpAgent.createSync({host: result.host, identity: new AnonymousIdentity(), retryTimes: 0,
  verifyQuerySignatures: true, shouldSyncTime: false, shouldFetchRootKey: false, fetch: transport});
agent.call = async () => {throw Error('updates forbidden');};
agent.syncTime = agent.syncTimeWithSubnet = async () => {throw Error('implicit time sync forbidden');};
async function slot(target, method, args, fn) {
  if (result.observations.length >= 35) throw Error('logical read limit');
  active = {target, method, until: Date.now()+30000};
  const row = {target, method, args, caller: result.caller, client_time: Date.now()};
  try { Object.assign(row, await fn()); row.ok = true; } catch(e) {row.ok = false; row.error = String(e);}
  result.observations.push(row); active = null;
  persist();
}
for (const target of Object.values(p.roles)) {
  for (const kind of ['module_hash','controllers']) {
    await slot(target, 'read_state', {kind}, async () => {
      const path = [new TextEncoder().encode('canister'), Principal.fromText(target).toUint8Array(), new TextEncoder().encode(kind)];
      const reply = await agent.readState(target, {paths:[path]});
      const cert = await Certificate.create({certificate: reply.certificate, rootKey: agent.rootKey, principal:{canisterId:Principal.fromText(target)}});
      const lookup = cert.lookup_path(path), bytes = lookupResultToBuffer(lookup);
      if (!bytes) throw Error(`certificate ${lookup.status}: ${kind} unavailable`);
      return {verification:'certified / SDK BLS, range and freshness checks; separate certificate per path',
        value: kind === 'controllers' ? Cbor.decode(bytes).map(b => Principal.fromUint8Array(b).toText()) : hex(bytes),
        subnet: cert.cert.delegation ? Principal.fromUint8Array(cert.cert.delegation.subnet_id).toText() : null};
    });
  }
}
const Account = IDL.Record({owner:IDL.Principal,subaccount:IDL.Opt(IDL.Vec(IDL.Nat8))});
for (const target of ledgers) {
  for (const method of ['icrc1_fee','icrc1_total_supply']) {
    await slot(target, method, [], async () => {
      const r = await agent.query(target, {methodName:method,arg:IDL.encode([],[])});
      if (r.status !== 'replied') throw Error(JSON.stringify(clean(r)));
      return {verification:'replica-signed query / not certified Ledger state', value:IDL.decode([IDL.Nat],new Uint8Array(r.reply.arg))[0].toString()};
    });
  }
}
for (const role of ['nns_staking_expectation','jupiter_relay','sns_governance']) {
  const a = accounts[role].account;
  await slot(icp, 'icrc1_balance_of', {account:a}, async () => {
    const r = await agent.query(icp, {methodName:'icrc1_balance_of',arg:IDL.encode([Account],[{owner:Principal.fromText(a.owner),subaccount:a.subaccountHex?[Buffer.from(a.subaccountHex,'hex')]:[]}])});
    if (r.status !== 'replied') throw Error(JSON.stringify(clean(r)));
    return {verification:'replica-signed query / not certified Ledger state',value:IDL.decode([IDL.Nat],new Uint8Array(r.reply.arg))[0].toString()};
  });
}
const queries = publicQueries(p.roles);
for (const [target,method,types,values,reply] of queries) {
  const args = method === 'get_upgrade_journal' ? {limit:20,offset:null} : [];
  await slot(target,method,args,async()=>{
    const r = await agent.query(target,{methodName:method,arg:IDL.encode(types,values)});
    if (r.status !== 'replied') throw Error(JSON.stringify(clean(r)));
    return {verification:'replica-signed query / application-reported state, not independent financial proof',value:clean(IDL.decode(reply,new Uint8Array(r.reply.arg))[0])};
  });
}
result.complete = result.observations.every(r=>r.ok);
result.finished = Date.now();
fs.writeFileSync(output, JSON.stringify(clean(result), null, 2)+'\n', {flag:'wx'});
fs.unlinkSync(output+'.partial');

}
if (process.argv[1] && new URL(import.meta.url).pathname === process.argv[1]) {
  const [profilePath, output] = process.argv.slice(2);
  await collect(profilePath, output);
}
