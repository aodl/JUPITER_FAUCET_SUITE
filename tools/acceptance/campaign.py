"""Finite offline verifier of saved acceptance exercise evidence; no network/signing.

Input records are normalized exports of canonical Ledger/Index/archive queries,
not application payment claims. Authentication belongs to the evidence collector;
this verifier hashes imports and never upgrades a signed query to certified state.
"""
from decimal import Decimal, localcontext
import hashlib
import json
import zlib

import economics
from ic import account, staking_subaccount
import maintenance
import observe
import profile as acceptance


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def reward_range(amount, timestamp, launch, compounded):
    """Same independent Decimal oracle tested in patron_math.rs, not a payment engine.

    Production Q96 result is at most one base unit below the real-formula floor.
    Whole halvings are exact. No nearest-value rounding or fee subtraction.
    """
    e = economics.load()
    days = max(0, timestamp - launch) // 86400
    if days // e['halving_days'] >= 64:
        return 0, 0
    with localcontext() as ctx:
        ctx.prec = 110
        exact = int(Decimal(amount) * (2 if compounded else 1) *
                    Decimal(2) ** (-Decimal(days) / e['halving_days']) / e['patron_icp_per_jup'])
    return max(0, exact - (days % e['halving_days'] != 0)), exact


def addr(value):
    # ICP query_blocks carries a32-byte legacy AccountIdentifier, not an
    # invertible owner/subaccount tuple. Compare its checked raw hash directly.
    if isinstance(value,str):
        raw=bytes.fromhex(value)
        if len(raw)!=32 or raw[:4]!=zlib.crc32(raw[4:]).to_bytes(4,'big'):
            raise ValueError('invalid canonical ICP AccountIdentifier')
        return raw.hex()
    return account(value['owner'], value.get('subaccountHex'))['explorerAccountHex']


def verify(manifest):
    if manifest.get('schema') != 1:
        raise ValueError('campaign evidence schema 1 required')
    catalog = maintenance.exercises(manifest['campaign_id'])
    known = {r['name']: r for r in catalog['exercises']}
    p = json.loads(acceptance.PROFILE.read_text())
    inputs = json.loads((acceptance.PROFILE.parent / 'candidate-inputs.json').read_text())
    accounts = json.loads((acceptance.PROFILE.parent / 'derived-accounts.json').read_text())
    allowed_targets = set(p['roles'].values()) | {acceptance.SYSTEM['ledger'], acceptance.SYSTEM['index'], acceptance.SYSTEM['governance']}
    allowed_methods = {'query_blocks', 'get_blocks', 'get_transactions', 'get_account_transactions',
                       'icrc1_balance_of', 'get_neuron', 'get_proposal', 'get_programme_status', 'get_patron_record', 'list_neurons'}
    sources = manifest['sources']
    if not 1 <= len(sources) <= 32 or not 1 <= len(manifest['exercises']) <= 5:
        raise ValueError('maximum 32 saved sources and six named exercises')
    # Only canonical Ledger reply callback ranges may introduce archives.
    archive_ranges = {}
    for src in sources:
        if src['target'] in (acceptance.SYSTEM['ledger'], p['roles']['sns_ledger']) and src['method'] in ('query_blocks','get_blocks','get_transactions'):
            for span in src['value'].get('archived_ranges', []):
                archive_ranges.setdefault(span['target'], []).append((src['target'],span['start'],span['end_exclusive']))
    source_map = {}
    for src in sources:
        if src['id'] in source_map or src['target'] not in allowed_targets | archive_ranges.keys() or src['method'] not in allowed_methods:
            raise ValueError('duplicate/unapproved canonical evidence source')
        if src['verification'] not in ('replica-signed-query', 'certified', 'local-PocketIC', 'local-fixture'):
            raise ValueError('explicit evidence verification classification required')
        # Archive targets require a canonical callback in a ledger response, not
        # a name supplied in an arbitrary manifest. No network discovery occurs.
        source_map[src['id']] = src
    windows = {}
    for window in manifest.get('windows', []):
        ledger = window['ledger']
        if ledger not in (acceptance.SYSTEM['ledger'], p['roles']['sns_ledger']) or ledger in windows:
            raise ValueError('one finite window per canonical ledger')
        start, end = window['start'], window['end_exclusive']
        if not 0 <= start <= end or end - start > 256:
            raise ValueError('each campaign window is at most 256 blocks')
        rows = []
        complete = True
        anchor = False
        for ref in window['sources']:
            src = source_map[ref]
            if (src['target'] != ledger and src['target'] not in archive_ranges) or src['method'] not in ('query_blocks', 'get_blocks', 'get_transactions'):
                raise ValueError('full canonical ledger range required, not filtered application history')
            page = src['value']
            # Exporter must retain unresolved callback ranges. Missing archives
            # are unavailable, never inferred zero. Archive exports may be merged
            # only after their raw callback responses are retained with the page.
            if src['target'] == ledger:
                anchor = True
                complete &= page['log_length'] >= end
            else:
                for row in page['transactions']:
                    if not any(owner == ledger and first <= row['block'] < last for owner,first,last in archive_ranges[src['target']]):
                        raise ValueError('archive record outside authentic callback range')
            if len(page['transactions']) > 256:
                raise ValueError('bounded canonical page required')
            rows.extend(page['transactions'])
        ids = [r['block'] for r in rows]
        complete &= anchor and len(ids) == len(set(ids)) and sorted(ids) == list(range(start, end))
        windows[ledger] = (rows, complete)
    output = []
    seen = set()
    def source(ref, target, method):
        row = source_map.get(ref)
        if not row or row['target'] != target or row['method'] != method:
            return None
        return row['value']
    for exercise in manifest['exercises']:
        name = exercise['name']
        if name not in known or name in seen:
            raise ValueError('unique known exercise name required')
        seen.add(name)
        checks = {}
        details = {}
        def check(label, condition=None):
            checks[label] = ('UNAVAILABLE_EVIDENCE' if condition is None else
                             'HEALTHY' if condition else 'INVARIANT_FAILURE')
        if name == 'harmless native proposal/vote':
            proposal = source(exercise.get('proposal_source'), p['roles']['sns_governance'], 'get_proposal')
            check('native_proposal_execution', None if proposal is None else
                  proposal['id'] == exercise['proposal_id'] and proposal['executed_timestamp_seconds'] > 0
                  and proposal['failed_timestamp_seconds'] == 0
                  and proposal.get('action',{}).get('kind')=='ManageSnsMetadata')
            check('decisive_native_ballot', None if proposal is None else
                  proposal.get('ballots',{}).get(staking_subaccount(p['operator'],1),{}).get('vote') == 1)
            check('historical_vote_signer', None)  # native ballots name neurons, not signing principals
        else:
            icp_rows, icp_complete = windows.get(acceptance.SYSTEM['ledger'], ([], False))
            deposits = [r for r in icp_rows if r['block'] == exercise['source_block']]
            operation = known[name]['operation']
            check('canonical_icp_window', True if icp_complete else None)
            if not icp_complete or len(deposits) != 1:
                check('source_transaction')
            else:
                deposit = deposits[0]
                expected_source = exercise['source_account']
                if name in ('direct liquid', 'direct 2x', 'Relay pot'):
                    participant = inputs['test_allocations'][2 if name == 'direct 2x' else 0]['owner']
                    check('approved_participant_source', addr(expected_source) == addr({'owner':participant}))
                else:
                    check('native_treasury_source', addr(expected_source) == addr(accounts['sns_governance']['account']))
                destination = accounts['nns_staking_expectation']['account']
                memo = operation.get('memo_hex')
                if name == 'bounded treasury stake':
                    memo = None
                check('source_transaction', deposit['kind'] == 'transfer' and
                      addr(deposit['from']) == addr(expected_source) and addr(deposit['to']) == addr(destination)
                      and deposit['amount'] == 100_000_000 and deposit['memo_hex'] == memo
                      and deposit['fee'] == exercise['approved_icp_fee_e8s']
                      and deposit['timestamp_ns'] == exercise['source_timestamp_ns'])
                if name == 'bounded treasury stake':
                    proposal = source(exercise.get('proposal_source'), p['roles']['sns_governance'], 'get_proposal')
                    check('executed_treasury_proposal', None if proposal is None else
                          proposal['id']==exercise['proposal_id']
                          and proposal['executed_timestamp_seconds'] > 0 and proposal['failed_timestamp_seconds'] == 0
                          and proposal.get('action',{}).get('kind')=='TransferSnsTreasuryFunds'
                          and proposal['action'].get('from_treasury')=='ICP'
                          and addr(proposal['action']['to'])==addr(destination)
                          and proposal['action'].get('amount_e8s')==deposit['amount']
                          and proposal['action'].get('memo') is None)
                    check('proposal_to_transfer_attribution', None)  # no canonical block ID in native proposal reply
                    check('nns_refresh', None)  # cached stake alone cannot attribute interleaved top-ups
                    check('no_patron_classification', None)  # finite jUP window cannot prove global absence
                else:
                    record = source(exercise.get('patron_source'), p['roles']['jupiter_sns_rewards'], 'get_patron_record')
                    if record is None:
                        check('frozen_patron_selection')
                    else:
                        compounded = name == 'direct 2x'
                        launch = record['launch_timestamp_seconds']
                        if type(launch) is not int or launch <= 0:
                            raise ValueError('explicit approved campaign launch clock required; no fixture/preparation clock substitution')
                        low, high = reward_range(deposit['amount'], deposit['timestamp_ns']//10**9, launch, compounded)
                        programme=source(exercise.get('programme_source'),p['roles']['jupiter_sns_rewards'],'get_programme_status')
                        check('frozen_programme_epoch', None if programme is None else programme['config']['version']==record['config_version']
                              and programme['config']['launch_timestamp_seconds']==launch and programme['config']['patron_enabled'] is True)
                        details['expected_reward_e8s_range'] = [low, high]
                        check('reward_oracle', low <= record['amount_e8s'] <= high)
                        neuron_id = staking_subaccount(inputs['test_allocations'][2]['owner'],21) if compounded else None
                        to = {'owner':p['roles']['sns_governance'],'subaccountHex':neuron_id} if compounded else (
                            {'owner':p['roles']['jupiter_relay']} if name=='Relay pot' else {'owner':participant})
                        expected_memo = (b'jUP-patron-v1' + deposit['block'].to_bytes(8,'big') + record['snapshot_id'].to_bytes(8,'big')).hex()
                        check('frozen_patron_selection', record['contribution_block']==deposit['block']
                              and record['qualifying_icp_e8s']==deposit['amount']
                              and record['commitment_timestamp_seconds']==deposit['timestamp_ns']//10**9
                              and record['commitment_memo_hex']==memo and addr(record['to'])==addr(to)
                              and record['memo_hex']==expected_memo)
                        rows, complete = windows.get(p['roles']['sns_ledger'], ([], False))
                        receipts = [r for r in rows if r.get('memo_hex')==expected_memo]
                        expected = dict(source_block=deposit['block'],destination=addr(to),amount=record['amount_e8s'],
                            fee=exercise['approved_jup_fee_e8s'],memo=expected_memo,dispatch_state='AssumedPaidUnverified' if record['outcome'] != 'Verified' else record['outcome'])
                        actual = [dict(source_block=deposit['block'],destination=addr(r['to']),amount=r['amount'],
                                       fee=r['fee'],memo=r['memo_hex']) for r in receipts if r['kind']=='transfer']
                        state = observe.reconcile(expected, dict(complete=complete,canonical=True,transactions=actual))
                        checks['relay_aggregate_pot' if name=='Relay pot' else 'jup_settlement'] = state
                        check('reserve_source_and_fee', None if not complete or not receipts else all(
                            addr(r['from'])==addr({'owner':p['roles']['jupiter_sns_rewards']})
                            and r['fee']==record['fee_e8s'] for r in receipts))
                        # Historic eligibility is not established by a later get_neuron.
                        # Keep selection unproven unless a separately reviewed frozen
                        # canonical evidence source exists; never trust journal recognition.
                        selection = source(exercise.get('selection_source'),p['roles']['sns_governance'],'get_neuron')
                        if name == 'Relay pot':
                            check('relay_membership_at_selection', None)
                        elif (selection is None or not isinstance(record.get('selection_at_ns'),int)
                              or not source_map[exercise['selection_source']]['verification'].startswith('local-')
                              or not source_map[exercise['patron_source']]['verification'].startswith('local-')
                              or source_map[exercise['selection_source']].get('captured_at_ns') != record.get('selection_at_ns')):
                            check('frozen_owner_eligibility', None)
                        else:
                            permissions = selection['permissions']
                            economic = [entry['principal'] for entry in permissions if 5 in entry['permissions']]
                            isolated = economic == [participant] and all(
                                entry['principal']==participant or set(entry['permissions']) <= {0,3,4,10} for entry in permissions)
                            expected_ids={staking_subaccount(participant,memo) for memo in ([21] if compounded else [10,11])}
                            eligible = isolated and all(type(code) is int and 0 <= code <= 10 for entry in permissions for code in entry['permissions']) and selection['id'] in expected_ids and selection['cached_neuron_stake_e8s']>selection.get('neuron_fees_e8s',0)
                            if compounded:
                                eligible &= selection['dissolve_delay_seconds']==economics.load()['year_seconds'] and selection['auto_stake_maturity'] is True
                            else:
                                eligible &= selection.get('auto_stake_maturity') is not True
                            check('frozen_owner_eligibility', eligible)
                            check('snapshot_freshness', 0 <= record['selection_at_ns']//10**9-record['snapshot_completed_at_seconds'] <= 172800)

                        if compounded:
                            refresh = source(exercise.get('refresh_source'),p['roles']['sns_governance'],'get_neuron')
                            if refresh is None:
                                check('subsequent_cached_stake_refresh', None)
                            elif refresh['id'] != neuron_id:
                                check('subsequent_cached_stake_refresh', False)
                            else:
                                before=source(exercise.get('refresh_before_source'),p['roles']['sns_governance'],'get_neuron')
                                b0=source_map.get(exercise.get('stake_balance_before_source'))
                                b1=source_map.get(exercise.get('stake_balance_after_source'))
                                refs=[source_map.get(exercise.get(k)) for k in ('refresh_before_source','stake_balance_before_source','refresh_source','stake_balance_after_source')]
                                balances=all(b and b['target']==p['roles']['sns_ledger'] and b['method']=='icrc1_balance_of'
                                    and addr(b.get('args',{}).get('account',{'owner':p['operator']}))==addr(to) for b in (b0,b1))
                                times=[r.get('captured_at_ns') if r else None for r in refs]
                                coherent=all(type(t) is int for t in times) and len(receipts)==1 and type(receipts[0].get('timestamp_ns')) is int
                                if before is None or not balances or not coherent or state!='HEALTHY':
                                    check('subsequent_cached_stake_refresh',None)
                                elif not (max(times[:2])<deposit['timestamp_ns'] and min(times[2:])>receipts[0]['timestamp_ns']):
                                    check('subsequent_cached_stake_refresh',None)
                                elif (before['id']!=neuron_id or before['cached_neuron_stake_e8s']!=b0['value']
                                      or b1['value']!=b0['value']+record['amount_e8s']):
                                    check('subsequent_cached_stake_refresh',None)  # interleaved/stale cohort, not an invented repair
                                else:
                                    checks['subsequent_cached_stake_refresh']='HEALTHY' if refresh['cached_neuron_stake_e8s']==b1['value'] else 'DELAYED'
                        if name=='Relay pot':
                            distribution = exercise.get('individual_receipt')
                            if distribution is None or not complete:
                                check('individual_relay_ledger_receipt', None)
                            else:
                                paid = [r for r in rows if r['block']==distribution['block']]
                                check('individual_relay_ledger_receipt', len(paid)==1 and paid[0]['kind']=='transfer'
                                      and addr(paid[0]['from'])==addr(to) and addr(paid[0]['to'])==addr(distribution['to'])
                                      and paid[0]['amount']==distribution['amount'] and paid[0]['fee']==distribution['fee']
                                      and paid[0]['memo_hex']==distribution['memo_hex'])
                            check('individual_relay_entitlement', None)  # immutable attribution requires its own frozen context
        # Disburser stage/age-at-pinning evidence is not enumerable by these APIs.
        # A caller may attach it for review, never turn missing stage evidence into 95/5 success.
        # Disburser is not an automatic side-effect claimed by a patron exercise.
        # Its own canonical stage/age evidence is listed as a campaign-level gap.
        status = ('FAIL' if 'INVARIANT_FAILURE' in checks.values() else
                  'INCOMPLETE' if any(v in ('UNAVAILABLE_EVIDENCE','DELAYED') for v in checks.values()) else 'PASS')
        output.append(dict(name=name,status=status,checks=checks,details=details))
    return dict(schema=1,fixture=any(s['verification'].startswith('local-') for s in sources),campaign_id=manifest['campaign_id'],input_sha256=digest(manifest),
                source_hashes={key:digest(row) for key,row in source_map.items()},
                status='FAIL' if any(r['status']=='FAIL' for r in output) else
                       'INCOMPLETE' if any(r['status']=='INCOMPLETE' for r in output) else 'PASS',
                provenance='Imported normalized canonical responses; authentication/export fidelity must be verified separately. Signed queries are not certified Ledger state.',
                no_network=True,no_signing=True,no_replay=True,
                duplicate_scope='Only the explicit complete finite Ledger windows; no global no-repeat/history claim',
                additional_limits={'disburser_base_age_split':'UNAVAILABLE_EVIDENCE: need pinned staging/age context plus three canonical ICP receipts, not current maturity or a patron journal'},exercises=output)
