#!/usr/bin/env python3
"""Offline acceptance-only proposals and candidate selection. Never executes IC calls."""
import argparse
import copy
import hashlib
import json
from pathlib import Path

import candid as cd
import profile as acceptance
import sns


def digest(value):
    return hashlib.sha256(value).hexdigest()


def plan(profile):
    acceptance.validate(profile)
    if profile['roles'] != json.loads(acceptance.PROFILE.read_text())['roles']:
        raise ValueError('maintenance plans require the fixed twelve acceptance assignments')
    if profile.get('maintenance', {}).get('policy') != 'proposed-acceptance-only':
        raise ValueError('explicit acceptance maintenance proposal required')
    roles, executor = profile['roles'], profile['operator']
    personal = acceptance.PROTECTED_PERSONAL_PRINCIPAL
    transitions = []
    for role in acceptance.SNS:
        native = roles['sns_governance' if role == 'sns_root' else 'sns_root']
        transitions.append(dict(role=role, target=roles[role], sender=executor,
            add_before_remove=dict(expected=[executor, personal],
                controllers=[executor, personal, native]),
            after_rehearsal=dict(expected=[executor, personal, native],
                controllers=[native, executor]),
            method='update_settings', approved=False))
    return dict(status='PROPOSED / NOT EXECUTABLE / separate owner approval',
        environment='acceptance', source_profile_sha256=digest(json.dumps(profile, sort_keys=True).encode()),
        executor=executor, controller_risk='Full arbitrary-code/state/asset authority; plaintext-at-rest key. Not upgrade-only or production custody.',
        framework=transitions,
        archive=dict(discovery='Ledger archives(), then physical controller/status verification; no arbitrary IDs',
            existing='Inspect physical controllers. Upgrade only if the executor is already authorised. A Root-only archive is BLOCKED: no supported live delegation route is established here; never impersonate Root or assume its controller can sign as it.',
            creation_policy='Genesis InitArgs: 4.5T per archive; controller_id=Root; more_controller_ids=[executor]. Before trigger require actual native creation cost <=1.5T and at least17.5T Ledger liquid cycles (4.5T allocation +10T native retained floor +existing3T operating margin). Unexpected archives at installation preflight STOP for actual-controller review.',
            initial_more_controller_ids=[executor],
            preserve='Keep controller_id=Root and all source-defined archive options; verify each created archive. Root discovery does not confer control.'),
        viewer=dict(principal=None, route='Anonymous public queries initially; private settings blocked until one separate low-privilege provider is approved.',
            proposed_settings='status_visibility=allowed_viewers [viewer]; log_visibility=allowed_viewers [viewer]; snapshot_visibility=controllers; no controller grant',
            default_log_memory_limit=4096, increase=None),
        applications='UNCHANGED: normal governed dapps; Faucet/Disburser self/Lifeline; immutable Relay children controllerless',
        gates=['Both active clocks remain null.', 'Fresh exact controller/module/settings/version/pending/chunk-store preconditions.',
               'No controller removal before governed application and framework rehearsals.',
               'Viewer provider and supported management interface must be verified separately.'])


def select(proposals):
    """Select from a saved canonical list_proposals reply, bounded, never execute payloads.

    Native NnsFunction discriminants are pinned in the reviewed IC source.
    Payload authentication/decoding is a separate mandatory gate.
    """
    rows = proposals['proposal_info']
    if len(rows) > 50:
        raise ValueError('maximum 50 proposal records per discovery pass')
    selected = []
    for row in rows:
        action = (row.get('proposal') or {}).get('action', {})
        execute = action.get('ExecuteNnsFunction', {})
        code = execute.get('nns_function')
        if code not in (30, 37):
            continue
        selected.append(dict(id=row['id'], status=row['status'],
            kind={30: 'AddSnsWasm', 37: 'InsertSnsWasmUpgradePathEntries'}[code],
            payload_sha256=digest(bytes.fromhex(execute['payload_hex'])),
            disposition='REVIEW ONLY: decode pinned payload, authenticate exact bytes/source and full tuple/path; never install or vote'))
    return selected


def write_plan(profile, output):
    result = plan(profile)
    out = Path(output)
    out.mkdir()
    did = Path(__file__).with_name('management.did')
    for row in result['framework']:
        for stage in ('add_before_remove', 'after_rehearsal'):
            value = cd.record(canister_id=cd.principal(row['target']),
                settings=cd.record(controllers='opt ' + cd.vector(
                    cd.principal(p) for p in row[stage]['controllers'])))
            name = row['role'] + '-' + stage
            data = cd.write_constructor(out, name, did, '(UpdateSettings)', value)
            row[stage]['argument_sha256'] = digest(data)
            row[stage]['argument_hex_file'] = name + '.hex'
    (out / 'plan.json').write_text(json.dumps(result, indent=2) + '\n')
    return result


def exercises(campaign_id):
    if not campaign_id or not campaign_id.replace('-', '').isalnum() or len(campaign_id) > 64:
        raise ValueError('explicit short campaign ID required')
    records = json.loads((acceptance.PROFILE.parent / 'proposed-operations-2026-11-15.json').read_text())
    rows = []
    for purpose in ('direct liquid', 'direct 2x'):
        operation = copy.deepcopy(next(r for r in records['funding'] if r.get('purpose') == purpose))
        rows.append(dict(name=purpose, proposed_max_debit_e8s=100_010_000, operation=operation,
            receipt='ICP source block + historic time/memo; frozen owner selection; exact liquid/neuron jUP Ledger block, sponsor fee; B additionally native cached-stake refresh'))
    relay = copy.deepcopy(rows[0]['operation'])
    profile = json.loads(acceptance.PROFILE.read_text())
    relay['memo_hex'] = profile['roles']['jupiter_relay'].encode().hex()
    relay['purpose'] = 'Relay staking endowment (not operational Relay wallet liquidity)'
    rows.append(dict(name='Relay pot', proposed_max_debit_e8s=100_010_000, operation=relay,
        funding='UNFUNDED: separate exercise budget/source approval; not added to the retained primary funding sheet',
        receipt='Canonical staking credit -> reserve-to-Relay aggregate pot block; individual child distributions separately unproven until canonical receipts are available'))
    rows.append(dict(name='harmless native proposal/vote', proposed_max_debit_e8s=0,
        operation=dict(target=profile['roles']['sns_governance'], method='manage_neuron',
            signer=profile['operator'], action='ManageSnsMetadata: acceptance-only description',
            native_permissions=[3,4,10], maximum_authorized_spend=0),
        receipt='Exact proposal ID, proposer/decisive ballot, execution; native jUP proposal rejection/fee reservation reviewed separately'))
    rows.append(dict(name='bounded treasury stake', proposed_max_debit_e8s=100_010_000,
        operation=copy.deepcopy(records['native_treasury_transfer']),
        receipt='Executed native proposal + real ICP staking receipt + later NNS refresh; no Faucet memo, patron reward or Relay pot'))
    return dict(campaign_id=campaign_id, dry_run=True, approved=False, automatic_execution=False,
        max_transfer_submission_attempts_per_exercise=1, retries=0,
        cooldown='No automatic recurrence. Close and reconcile the campaign before separately approving another.',
        duplicate_prevention='Before any later submission, persist immutable campaign/payload hash, ledger created_at_time, source/nonce and attempt marker outside Git. Unknown result requires canonical reconciliation, never replay. This tool never submits.',
        exercises=rows)


def candidate(lock_path, directory, output):
    """Verify only this source-reviewed fixture, not an arbitrary supplied lock."""
    source = Path(lock_path).read_bytes()
    reviewed = acceptance.PROFILE.parent / 'maintenance-candidate.json'
    if source != reviewed.read_bytes():
        raise ValueError('unreviewed candidate lock: exact official source/proposal/hash review required first')
    lock = json.loads(source)
    interfaces = sns.verify_artifacts(lock, directory)
    baseline = json.loads((acceptance.PROFILE.parent / 'sns-framework.json').read_text())
    profile = json.loads(acceptance.PROFILE.read_text())
    acceptance.validate(profile)
    changed = [r for r in baseline['components']
               if baseline['components'][r]['raw_sha256'] != lock['components'][r]['raw_sha256']]
    if not changed:
        raise ValueError('no changed component')
    out = Path(output)
    out.mkdir()
    report = dict(kind='OFFLINE MECHANISM FIXTURE / not a selected NNS proposal',
        executable=False, live_approved=False, changed=changed, baseline=baseline, candidate=lock,
        profile=plan(profile),
        upgrade_mode='upgrade only / never reinstall; standard management or chunked installation',
        preconditions=['exact physical package hashes and controller/viewer sets',
            'Governance deployed/target/pending versions and upgrade journal; advancement false',
            'authentic Ledger archives/ranges/controllers; obligations quiesced and independently reconciled',
            'typed upgrade arguments, migration prerequisites and reviewed chunk-store contents'],
        metadata='Direct install may leave Governance deployed/target metadata unchanged: record drift, block blessed automation.',
        recovery='Candidate experiment epoch; no automatic downgrade, snapshot rollback or blessed-route recovery claimed.',
        governance_guard='Unreviewed Governance hash must stop Jupiter schema-sensitive workers; no allow-any bypass.',
        writes=[], nns_vote=None)
    for role, did in interfaces.items():
        (out / (role + '.did')).write_bytes(did)
    (out / 'review.json').write_text(json.dumps(report, indent=2) + '\n')
    return report


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    p = sub.add_parser('plan')
    p.add_argument('--profile', required=True)
    p.add_argument('--output', help='Offline typed controller arguments only; never submitted')
    p = sub.add_parser('select')
    p.add_argument('--saved-proposals', required=True)
    p = sub.add_parser('candidate')
    p.add_argument('--lock', required=True)
    p.add_argument('--artifacts', required=True)
    p.add_argument('--output', required=True)
    p = sub.add_parser('viewer-settings')
    p.add_argument('--principal', required=True)
    p.add_argument('--output', required=True)
    p = sub.add_parser('verify-campaign')
    p.add_argument('--evidence', required=True, help='Saved finite canonical export; no network')
    p = sub.add_parser('exercises')
    p.add_argument('--campaign-id', required=True)
    args = parser.parse_args()
    if args.command == 'plan':
        profile = json.loads(Path(args.profile).read_text())
        print(json.dumps(write_plan(profile, args.output) if args.output else plan(profile), indent=2))
    elif args.command == 'select':
        print(json.dumps(select(json.loads(Path(args.saved_proposals).read_text())), indent=2))
    elif args.command == 'viewer-settings':
        import viewer
        viewer.settings(args.principal, args.output)
    elif args.command == 'verify-campaign':
        import campaign
        raw = Path(args.evidence).read_bytes()
        if len(raw) > 4 * 1024 * 1024:
            raise ValueError('campaign evidence exceeds 4MiB')
        report = campaign.verify(json.loads(raw))
        print(json.dumps(report, indent=2))
        raise SystemExit({'PASS': 0, 'INCOMPLETE': 2, 'FAIL': 3}[report['status']])
    elif args.command == 'exercises':
        print(json.dumps(exercises(args.campaign_id), indent=2))
    else:
        candidate(args.lock, args.artifacts, args.output)
