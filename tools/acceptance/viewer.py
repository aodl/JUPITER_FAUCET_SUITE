"""Optional restricted-viewer observation; DFX provider only after separate approval.

No key handling, fallback identity, wallet, setting mutation or scheduler. The
caller must run in an existing host context containing ONLY the approved viewer.
DFX transport/support traffic is not wire-metered: approval is for 24 logical
read-only calls plus normal verification, not the anonymous adapter's 128-HTTP cap.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

import candid as cd
import observe
import profile as acceptance


def validate(principal):
    p = json.loads(acceptance.PROFILE.read_text())
    acceptance.isolated_principal(principal)
    inputs = json.loads((acceptance.PROFILE.parent / 'candidate-inputs.json').read_text())
    forbidden = set(p['roles'].values()) | {p['operator'], acceptance.PROTECTED_PERSONAL_PRINCIPAL,
        *(n['owner'] for n in inputs['test_allocations'])}
    if principal in forbidden:
        raise ValueError('viewer must be separate from executor, personal, participants and protocol')
    return p


def settings(principal, output):
    p = validate(principal)
    out = Path(output); out.mkdir()
    rows = []
    for role, target in p['roles'].items():
        value = cd.record(canister_id=cd.principal(target), settings=cd.record(
            status_visibility='opt variant { allowed_viewers = vec { '+cd.principal(principal)+' } }',
            log_visibility='opt variant { allowed_viewers = vec { '+cd.principal(principal)+' } }',
            snapshot_visibility='opt variant { controllers }'))
        data=cd.write_constructor(out,role,Path(__file__).with_name('management.did'),'(UpdateSettings)',value)
        rows.append(dict(role=role,target=target,method='update_settings',argument_sha256=hashlib.sha256(data).hexdigest(),
                         controllers='UNCHANGED / no grant',approved=False))
    observe.atomic(out/'plan.json',dict(viewer=principal,approved=False,operations=rows,
        blocked='Separate owner approval; targets without an executable controller route stay unavailable. Never alter immutable children.'))


def commands(approval):
    principal=approval['principal']; p=validate(principal)
    alias=approval['dfx_identity_alias']
    if (approval.get('approved') is not True or approval.get('restricted_host_confirmed') is not True
            or approval.get('network')!='ic' or approval.get('dfx_version')!='0.31.0'
            or not isinstance(alias,str) or not alias.replace('_','').replace('-','').isalnum()
            or alias in ('codex_local','anonymous') or approval.get('targets') != list(p['roles'].values())):
        raise ValueError('exact dedicated provider/context/24-read approval required')
    result=[]
    did=Path(__file__).with_name('management.did').resolve()
    for target in approval['targets']:
        for method in ('canister_status','fetch_canister_logs'):
            command=['dfx','--identity',alias,'canister','call','aaaaa-aa',method,
                     '(record { canister_id = principal "'+target+'" })',
                     '--network','ic','--candid',str(did),'--output','raw',
                     '--update' if method=='canister_status' else '--query']
            result.append(dict(target=target,method=method,command=command))
    return result


def summarize(rows, previous=None):
    expected=list(json.loads(acceptance.PROFILE.read_text())['roles'].values())
    if len(rows)>24 or any(r['target'] not in expected or r['method'] not in ('canister_status','fetch_canister_logs') for r in rows):
        raise ValueError('viewer evidence outside the exact read registry')
    prior=(previous or {}).get('logs',{})
    logs={}; alerts=[]; cycles={}; tails={}
    for row in rows:
        target=row['target']
        if not row.get('ok'):
            alerts.append(dict(target=target,category='UNAVAILABLE_EVIDENCE',reason=row.get('error','read denied'))); continue
        if row['method']=='canister_status':
            value=row['value'];cycles[target]={k:value[k] for k in ('cycles','reserved_cycles','memory_size','settings','status','module_hash')}
        else:
            records=row['value']['canister_log_records']
            ids=[int(r['idx']) for r in records]
            if len(ids)!=len(set(ids)) or ids!=sorted(ids):
                raise ValueError('invalid log index order/duplicates')
            last=prior.get(target)
            if ids and ((last is None and ids[0]>0) or (last is not None and (ids[0]>last+1 or ids[-1]<last))):
                alerts.append(dict(target=target,category='UNAVAILABLE_EVIDENCE',reason='log rollover/gap or reset; explicit new epoch review required'))
            if not ids and last is not None:
                alerts.append(dict(target=target,category='UNAVAILABLE_EVIDENCE',reason='previous log records absent; no inferred continuity'))
            if last is not None and target in (previous or {}).get('tail_hashes',{}):
                overlap=next((r for r in records if int(r['idx'])==last),None)
                if overlap is not None and observe.sha(overlap)!=previous['tail_hashes'][target]:
                    alerts.append(dict(target=target,category='INVARIANT_FAILURE',reason='same log index changed; reset/epoch review required'))
            logs[target]=max(ids) if ids else last
            tails[target]=observe.sha(records[-1]) if records else (previous or {}).get('tail_hashes',{}).get(target)
    expected=list(json.loads(acceptance.PROFILE.read_text())['roles'].values())
    complete=len(rows)==24 and {(r['target'],r['method']) for r in rows}=={
        (t,m) for t in expected for m in ('canister_status','fetch_canister_logs')} and all(r.get('ok') for r in rows)
    if previous and any(target in previous.get('cycles',{}) and
        (value['module_hash']!=previous['cycles'][target]['module_hash'] or value['settings'].get('controllers')!=previous['cycles'][target]['settings'].get('controllers'))
        for target,value in cycles.items()):
        alerts.append(dict(category='BLOCKED',reason='module/controller epoch changed'))
    return dict(logs=logs,tail_hashes=tails,cycles=cycles,alerts=alerts,complete=complete,
                checkpoint_eligible=complete and not alerts,
                verification='DFX verified replicated status / replica-signed log queries; logs are diagnostics, never canonical payment evidence')


def collect(approval, persist=None):
    todo=commands(approval)
    env=dict(os.environ,DFX_VERSION='0.31.0',DFX_IDENTITY=approval['dfx_identity_alias'])
    # Official DFX0.31 environment.rs enables signatures unless this override
    # is nonempty; never inherit an operator's diagnostic disable flag.
    env.pop('DFX_DISABLE_QUERY_VERIFICATION', None)
    def local(args):
        return subprocess.check_output(args,env=env,text=True,timeout=10).strip()
    if local(['dfx','--version'])!='dfx 0.31.0': raise ValueError('pinned DFX required')
    if local(['dfx','--identity',approval['dfx_identity_alias'],'identity','get-principal'])!=approval['principal']:
        raise ValueError('provider public principal mismatch; no network reads')
    rows=[];started=time.monotonic()
    for call in todo:
        row={k:call[k] for k in ('target','method')};row.update(caller=approval['principal'],client_time_ns=time.time_ns())
        try:
            if time.monotonic()-started>750: raise ValueError('overall viewer deadline')
            raw=subprocess.check_output(call['command'],env=env,text=True,timeout=30,stderr=subprocess.PIPE).strip()
            if len(raw)>1_048_576: raise ValueError('response exceeds512KiB')
            # SDK type projection only: no HTTP or identity loaded by this mode.
            value=subprocess.check_output(['node',str(Path(__file__).with_name('observe.mjs')),
                '--decode-management',call['method']],input=raw,text=True,timeout=10)
            row.update(ok=True,value=json.loads(value),raw_hex=raw)
        except (ValueError,subprocess.SubprocessError) as error:
            row.update(ok=False,error=type(error).__name__+': permitted read unavailable')
        rows.append(row)
        if persist:
            persist(dict(caller=approval['principal'],network='ic',observations=rows,complete=False))
    return dict(caller=approval['principal'],network='ic',observations=rows,
                transport='24 logical DFX requests maximum; SDK verification/support transport not individually metered; no wrapper retries')
