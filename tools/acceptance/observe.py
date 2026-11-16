#!/usr/bin/env python3
"""One-shot acceptance evidence/report command. No scheduler activation or writes to IC."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

import profile as acceptance

CATEGORIES = ('HEALTHY', 'EXPECTED_IDLE', 'DELAYED', 'BLOCKED', 'INSUFFICIENT_FUNDS',
              'UNAVAILABLE_EVIDENCE', 'INVARIANT_FAILURE', 'NOT_ENABLED')


def sha(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()


def slots(profile):
    accounts = json.loads((acceptance.PROFILE.parent / 'derived-accounts.json').read_text())
    rows = [(target, 'read_state', dict(kind=kind))
            for target in profile['roles'].values() for kind in ('module_hash', 'controllers')]
    rows += [(target, method, []) for target in (acceptance.SYSTEM['ledger'], profile['roles']['sns_ledger'])
             for method in ('icrc1_fee', 'icrc1_total_supply')]
    rows += [(acceptance.SYSTEM['ledger'], 'icrc1_balance_of', dict(account=accounts[role]['account']))
             for role in ('nns_staking_expectation', 'jupiter_relay', 'sns_governance')]
    rows += [(profile['roles']['sns_governance'], method, [] if method != 'get_upgrade_journal' else dict(limit=20, offset=None))
             for method in ('get_running_sns_version', 'get_nervous_system_parameters', 'get_upgrade_journal')]
    rows += [(profile['roles']['jupiter_sns_rewards'], 'get_programme_status', [])]
    return rows


def reconcile(expected, receipt):
    """Exact receipt arithmetic, not entitlement calculation or a journal verifier.

    Only complete canonical history can establish absence/duplicates. Imported
    fixtures exercise arithmetic; they are never relabelled live observations.
    """
    if expected.get('dispatch_state') == 'AssumedPaidUnverified':
        return 'UNAVAILABLE_EVIDENCE'
    if not receipt.get('complete') or not receipt.get('canonical'):
        return 'UNAVAILABLE_EVIDENCE'
    if expected.get('owner_snapshot_fresh') is False:
        return 'UNAVAILABLE_EVIDENCE'
    rows = receipt.get('transactions', [])
    if len(rows) != 1:
        return 'INVARIANT_FAILURE'
    row = rows[0]
    if any(row.get(k) != expected.get(k) for k in ('source_block', 'destination', 'amount', 'fee', 'memo')):
        return 'INVARIANT_FAILURE'
    if expected.get('compounded') and not receipt.get('cached_stake_refreshed'):
        return 'DELAYED'
    return 'HEALTHY'


def report(profile, evidence, previous, now, interval):
    acceptance.validate(profile)
    if evidence.get('network') != 'ic' or evidence.get('caller') != '2vxsx-fae':
        raise ValueError('anonymous acceptance evidence required')
    rows = evidence.get('observations', [])
    if len(rows) > 35 or len(evidence.get('traffic', [])) > 128:
        raise ValueError('read budget exceeded')
    allowed = {sha(row) for row in slots(profile)}
    seen = set()
    for row in rows:
        key = sha((row['target'], row['method'], row['args']))
        if key not in allowed or key in seen:
            raise ValueError('evidence outside read-only registry')
        seen.add(key)
    alerts = []
    if previous and (previous.get('schema') != 1 or not {
            'at', 'physical', 'configuration', 'profile_sha256'} <= previous.keys()):
        raise ValueError('checkpoint revision mismatch; preserve it for explicit epoch review')
    if previous and now < previous['at']:
        raise ValueError('non-monotonic checkpoint')
    if previous and now - previous['at'] > 2 * interval:
        alerts.append(dict(category='DELAYED', reason='missed observation intervals; no inferred continuity'))
    complete = evidence.get('complete') is True and len(rows) == len(allowed) and all(r.get('ok') for r in rows)
    # Do not include client timestamps in module/config epoch comparison.
    physical = sha([(r['target'], r['args'], r.get('value')) for r in rows if r['method'] == 'read_state'])
    configuration = sha([(r['method'], r.get('value', {}).get('config') if r['method'] == 'get_programme_status' else r.get('value'))
                         for r in rows if r.get('ok') and r['method'] in {
                             'get_programme_status', 'get_running_sns_version', 'get_nervous_system_parameters'}])
    if complete and previous and previous['physical'] != physical:
        alerts.append(dict(category='BLOCKED', reason='module/controller epoch changed; review before continuing financial cohorts'))
    if previous and previous['profile_sha256'] != sha(profile):
        alerts.append(dict(category='BLOCKED', reason='profile/configuration epoch changed; explicit review required'))
    if complete and previous and previous['configuration'] != configuration:
        alerts.append(dict(category='BLOCKED', reason='programme/framework version epoch changed; explicit review required'))
    if not complete:
        alerts.append(dict(category='UNAVAILABLE_EVIDENCE', reason='partial/failed read set; checkpoint not advanced'))
    for row in rows:
        if not row.get('ok'):
            continue
        if row['method'] == 'get_nervous_system_parameters' and row['value'].get('automatically_advance_target_version') != [False]:
            alerts.append(dict(category='BLOCKED', reason='automatic target advancement not explicitly false'))
        if row['method'] == 'get_running_sns_version' and row['value'].get('pending_version'):
            alerts.append(dict(category='BLOCKED', reason='native framework upgrade already pending; false advancement does not cancel it'))
        if row['method'] == 'get_programme_status' and row['value'].get('last_error'):
            alerts.append(dict(category='BLOCKED', reason='application reports error: inspect saved evidence; no payment replay'))
    # Public balance samples do not independently prove individual payouts.
    coverage = dict(public_read_availability='HEALTHY' if complete else 'UNAVAILABLE_EVIDENCE',
        expected_modules_controllers='UNAVAILABLE_EVIDENCE',
        cycles_logs_headroom='UNAVAILABLE_EVIDENCE', direct_patron='UNAVAILABLE_EVIDENCE',
        compounding_refresh='UNAVAILABLE_EVIDENCE', relay_individual_receipts='UNAVAILABLE_EVIDENCE',
        treasury_nns_refresh='UNAVAILABLE_EVIDENCE', maturity_95_5='UNAVAILABLE_EVIDENCE',
        participation_exclusions='UNAVAILABLE_EVIDENCE', io_settlement='NOT_ENABLED')
    return dict(schema=1, at=now, fixture=evidence.get('kind') != 'anonymous-sdk-collection',
        status='BLOCKED' if any(a['category']=='BLOCKED' for a in alerts) else 'UNAVAILABLE_EVIDENCE',
        collector_source_sha256=sha({suffix: Path(__file__).with_suffix(suffix).read_bytes().hex()
                                     for suffix in ('.py', '.mjs')}),
        profile_sha256=sha(profile), evidence_sha256=sha(evidence), physical=physical, configuration=configuration,
        complete=complete, coverage=coverage, alerts=alerts,
        reads=len(rows), verification='See per-source provenance; signed queries are not certified Ledger state.',
        transport_attempts=len(evidence['traffic']) if 'traffic' in evidence else None,
        checkpoint_eligible=complete and not any(a['category']=='BLOCKED' for a in alerts))


def atomic(path, value):
    fd, name = tempfile.mkstemp(dir=path.parent, prefix='.next-')
    try:
        with os.fdopen(fd, 'w') as stream:
            json.dump(value, stream, indent=2); stream.write('\n'); stream.flush(); os.fsync(stream.fileno())
        os.replace(name, path)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def run(profile_path, output, fixture=None, collect=False, interval=900):
    profile = json.loads(Path(profile_path).read_text())
    acceptance.validate(profile)
    if collect and profile['roles'] != json.loads(acceptance.PROFILE.read_text())['roles']:
        raise ValueError('collector requires the twelve assigned acceptance targets')
    if interval <= 0:
        raise ValueError('positive observation interval required')
    out = Path(output).resolve()
    if out.is_relative_to(acceptance.REPO) and not out.is_relative_to(acceptance.REPO / 'target'):
        raise ValueError('observations must be outside tracked source (or under ignored target/)')
    out.mkdir(parents=True, exist_ok=True, mode=0o700)
    with (out / '.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if collect:
            if fixture:
                raise ValueError('choose one evidence source')
            raw = out / ('raw-' + str(time.time_ns()) + '.json')
            try:
                subprocess.run(['node', str(Path(__file__).with_suffix('.mjs')), str(Path(profile_path).resolve()), str(raw)],
                               check=True, timeout=210)
                evidence = json.loads(raw.read_text())
            except subprocess.SubprocessError as error:
                partial = Path(str(raw) + '.partial')
                evidence = json.loads(partial.read_text()) if partial.exists() else dict(
                    kind='anonymous-sdk-collection', network='ic', caller='2vxsx-fae', observations=[])
                evidence.update(complete=False, error=str(error))
        else:
            if not fixture:
                raise ValueError('explicit --fixture or --collect required; never choose network silently')
            evidence = json.loads(Path(fixture).read_text())
            evidence['kind'] = 'local-fixture-not-live'
        checkpoint = out / ('fixture-checkpoint.json' if not collect else 'checkpoint.json')
        previous = json.loads(checkpoint.read_text()) if checkpoint.exists() else None
        result = report(profile, evidence, previous, int(time.time()), interval)
        history = out / 'reports'
        history.mkdir(exist_ok=True, mode=0o700)
        atomic(history / (str(time.time_ns()) + '.json'), result)
        atomic(out / 'report.json', result)
        atomic(out / 'alerts.json', result['alerts'])
        text = ['# Jupiter acceptance observation', '', f"Fixture: {result['fixture']}; complete: {result['complete']}", '']
        text.extend(f'- {name}: {status}' for name, status in result['coverage'].items())
        text.extend(f"- {a['category']}: {a['reason']}" for a in result['alerts'])
        (out / 'report.md').write_text('\n'.join(text)+'\n')
        if result['checkpoint_eligible']:
            atomic(checkpoint, result)
        return 0 if result['checkpoint_eligible'] else 2


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', required=True)
    parser.add_argument('--output', required=True)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument('--fixture')
    source.add_argument('--collect', action='store_true', help='Requires separate standing-read approval; anonymous public reads only')
    parser.add_argument('--interval-seconds', type=int, default=900)
    args = parser.parse_args()
    try:
        raise SystemExit(run(args.profile, args.output, args.fixture, args.collect, args.interval_seconds))
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        parser.exit(2, f'BLOCKED: {error}\n')
