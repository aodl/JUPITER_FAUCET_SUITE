#!/usr/bin/env python3
"""Offline acceptance genesis and native Governance recipes. Never calls the IC.

Unanswered decisions are retained in source; only explicit approved inputs can
materialize reviewed-acceptance constructors. Local rehearsal uses a separate
entry point and owned identities, never an implied operator approval.
"""
import argparse
import base64
import copy
import hashlib
import json
from pathlib import Path
import subprocess

import candid as cd
import economics
from ic import account, staking_subaccount
import prepare
import profile as acceptance
import sns


def natural(value, name, positive=False):
    if type(value) is not int or not (int(positive) <= value < 2**64):
        raise ValueError(f'explicit nat64 required: {name}')
    return value


def parameters():
    """Reviewed direct-install parameters, independent of contested fixtures."""
    e = economics.load()
    numbers = {
        'reject_cost_e8s': e['proposal_rejection_e8s'],
        'neuron_minimum_stake_e8s': e['minimum_neuron_stake_e8s'],
        'transaction_fee_e8s': e['transfer_fee_e8s'],
        'max_proposals_to_keep_per_action': 100,
        'initial_voting_period_seconds': e['jupiter_voting_seconds'],
        'wait_for_quiet_deadline_increase_seconds': e['wait_for_quiet_seconds'],
        'max_number_of_neurons': 1000,
        'neuron_minimum_dissolve_delay_to_vote_seconds': e['minimum_voting_dissolve_delay_seconds'],
        'max_followees_per_function': 15,
        'max_dissolve_delay_seconds': e['max_dissolve_delay_seconds'],
        'max_neuron_age_for_age_bonus': e['max_neuron_age_for_age_bonus'],
        'max_number_of_proposals_with_ballots': 700,
        'max_number_of_principals_per_neuron': 5,
        'max_dissolve_delay_bonus_percentage': e['dissolve_bonus_percentage'],
        'max_age_bonus_percentage': e['age_bonus_percentage'],
    }
    fields = {key: 'opt ' + str(value) for key, value in numbers.items()}
    fields.update(default_followees='opt record { followees = vec {} }',
        neuron_claimer_permissions='opt record { permissions = vec { 1;2;3;4;5;6;7;8;9;10 } }',
        neuron_grantable_permissions='opt record { permissions = vec { 1;2;3;4;5;6;7;8;9;10 } }',
        voting_rewards_parameters='opt ' + cd.record(initial_reward_rate_basis_points='opt 0',
            final_reward_rate_basis_points='opt 0', reward_rate_transition_duration_seconds='opt 0',
            round_duration_seconds='opt ' + str(e['native_reward_round_seconds'])),
        maturity_modulation_disabled='opt true', automatically_advance_target_version='opt false')
    return cd.record(**fields)


def neuron(controller, memo, stake, delay, permissions):
    return dict(controller=controller, memo=memo, stake=stake, dissolve_delay=delay,
        permissions=[dict(principal=controller, permissions=permissions)],
        voting_power_percentage_multiplier=100, auto_stake_maturity=False,
        vesting_period_seconds=0)


def render(profile, decisions, local=False):
    """Build exact economics with an explicit executable voting route.

    Vote, SubmitProposal and ManagePrincipals suffice for ordinary operation and
    later handover. Neither economic control nor a human is silently selected.
    """
    required = {'roles_confirmed', 'operator', 'voting_authority',
        'acceptance_start_timestamp_seconds', 'genesis_timestamp_seconds',
        'test_allocations', 'relay_surplus_neuron_recipients', 'bootstrap_disburser', 'approved'}
    if set(decisions) != required:
        raise ValueError('unexpected/missing operator decision fields')
    if not local and (decisions['approved'] is not True or decisions['roles_confirmed'] is not True):
        raise ValueError('operator approval and role confirmation are unresolved')
    authority = decisions['voting_authority']
    if not isinstance(authority, dict) or set(authority) != {'principal', 'kind', 'executable_route'}:
        raise ValueError('voting authority and executable route are unresolved')
    if authority['kind'] not in ('real-nnx', 'temporary-acceptance') or not isinstance(authority['executable_route'], str) or not authority['executable_route'].strip():
        raise ValueError('explicit real NNX route or approved temporary acceptance route required')
    acceptance.isolated_principal(authority['principal'])
    p = copy.deepcopy(profile)
    p['operator'] = decisions['operator']
    p['relay_surplus_neuron_recipients'] = decisions['relay_surplus_neuron_recipients']
    acceptance.validate(p)
    if authority['kind'] == 'temporary-acceptance' and authority['principal'] != p['operator']:
        raise ValueError('approved temporary acceptance voter must be the verified operator')
    if authority['principal'] in p['roles'].values():
        raise ValueError('voting executor must have its own executable route, outside the twelve roles')
    for name in ('genesis_timestamp_seconds', 'acceptance_start_timestamp_seconds'):
        natural(decisions[name], name, positive=True)
    if p['relay_surplus_neuron_recipients'] is None:
        raise ValueError('Relay surplus decision is unresolved')
    allocations = decisions['test_allocations']
    if not isinstance(allocations, list) or not allocations:
        raise ValueError('20,000-jUP test bucket allocation is unresolved')
    e = economics.load()
    if (len(allocations) != 4 or any(entry.get('kind') != 'neuron' or entry.get('amount_e8s') != 5000 * 10**8
            for entry in allocations)
            or [entry.get('dissolve_delay_seconds') for entry in allocations] != [0, e['year_seconds'], 0, e['year_seconds']]
            or allocations[0].get('owner') != allocations[1].get('owner')
            or allocations[2].get('owner') != allocations[3].get('owner')
            or allocations[0].get('owner') == allocations[2].get('owner')
            or len({entry.get('memo') for entry in allocations}) != 4):
        raise ValueError('approved A/B allocation requires four distinct-memo 5000-jUP zero/year neurons')
    rewards = p['roles']['jupiter_sns_rewards']
    neurons = [neuron(authority['principal'], 1, e['governance_stake_e8s'], e['year_seconds'], [2, 3, 4]),
        neuron(rewards, 2, e['reserve_transport_e8s'], 0, [5]),
        neuron(rewards, 3, e['proposer_stake_e8s'], e['year_seconds'], [3])]
    balances = []
    total = 0
    for entry in allocations:
        if set(entry) != {'owner', 'amount_e8s', 'kind', 'memo', 'dissolve_delay_seconds'}:
            raise ValueError('test allocation requires explicit owner, amount, kind, memo and delay')
        owner = acceptance.isolated_principal(entry['owner'])
        if owner in p['roles'].values():
            raise ValueError('test bucket must be separate from protocol canister roles')
        amount = natural(entry['amount_e8s'], 'test allocation', positive=True)
        total += amount
        if entry['kind'] == 'liquid' and entry['memo'] is None and entry['dissolve_delay_seconds'] is None:
            balances.append(dict(owner=owner, amount=amount))
        elif entry['kind'] == 'neuron':
            memo = natural(entry['memo'], 'test neuron memo')
            if amount < e['minimum_neuron_stake_e8s'] or entry['dissolve_delay_seconds'] not in (0, e['year_seconds']):
                raise ValueError('test neurons require minimum stake and zero/one-year examples')
            neurons.append(neuron(owner, memo, amount, entry['dissolve_delay_seconds'], list(range(1, 11))))
        else:
            raise ValueError('invalid test allocation kind or unused fields')
    if total != e['sale_allocation_e8s']:
        raise ValueError('test allocation must sum to exactly the 20,000-jUP bucket')
    # Operational metadata: no claim of a sale or production NNX implementation.
    logo = 'data:image/png;base64,' + base64.b64encode(
        (acceptance.PROFILE.parent / 'jupiter-logo.png').read_bytes()).decode('ascii')
    p['genesis'] = dict(purpose='local-fixture-not-for-deployment' if local else 'reviewed-acceptance',
        token_name=e['token_name'], token_symbol=e['token_symbol'], decimals=e['decimals'],
        fee=e['transfer_fee_e8s'], minimum_neuron_stake=e['minimum_neuron_stake_e8s'],
        parameters=parameters(), neurons=neurons, balances=balances,
        total_supply=e['initial_supply_e8s'], archive_cycles=1_000_000_000_000,
        metadata=dict(name='Jupiter acceptance', description='Direct-installed operational acceptance; no sale history.',
            url='https://' + p['roles']['jupiter_faucet_frontend'] + '.icp0.io/', logo=logo))
    checked = acceptance.validate(p)
    ids = {name: staking_subaccount(n['controller'], n['memo']) for name, n in
           zip(('governance', 'reserve_transport', 'proposer'), neurons[:3])}
    policy = dict(version=1, launch_timestamp_seconds=None, patron_enabled=False,
        protocol_governance_neuron=ids['governance'], excluded_jup_neurons=list(ids.values()),
        excluded_icp_accounts=[], reserve_subaccount=None, reserve_transport_neuron=ids['reserve_transport'],
        snapshot_max_age_seconds=172800, automation=dict(proposer_neuron=ids['proposer'], targets=[],
            treasury_enabled=False, treasury_retain_icp_e8s=0, treasury_max_proposal_icp_e8s=0))
    # Validate policy against Rust's checked-in Candid before creating output.
    did = acceptance.REPO / 'canisters/sns-rewards/jupiter_sns_rewards.did'
    cd.encode_checked(did, '(ProgrammeConfig)', '(' + prepare.programme_arguments(checked, policy) + ')')
    return p, ids, policy


def manage_value(subaccount, command):
    return cd.record(subaccount=cd.blob(bytes.fromhex(subaccount)), command='opt ' + command)


def proposal(action, title):
    return 'variant { MakeProposal = ' + cd.record(title=cd.text(title), summary=cd.text(title),
        url=cd.text('https://github.com/aodl/JUPITER_FAUCET_SUITE'), action='opt ' + action) + ' }'


def upgrade_action(canister, wasm, args):
    if len(wasm) > 1_800_000:
        raise ValueError('upgrade package requires separately reviewed native chunked proposal')
    return 'variant { UpgradeSnsControlledCanister = ' + cd.record(
        canister_id='opt ' + cd.principal(canister), new_canister_wasm=cd.blob(wasm),
        mode='opt (3 : int32)', canister_upgrade_arg='opt ' + cd.blob(args)) + ' }'


def recipes(p, ids, policy, framework_did, out):
    """State-preserving upgrade is the existing authorized configuration API.

    No invented configure method or unvalidated generic nervous-system function.
    """
    out = Path(out)
    out.mkdir()
    def write(name, command, sub=ids['governance']):
        return cd.write_constructor(out, name, framework_did, '(ManageNeuron)', manage_value(sub, command))
    write('disable-auto-target', proposal('variant { ManageNervousSystemParameters = record { automatically_advance_target_version = opt false } }',
        'Acceptance: require reviewed framework upgrades'))
    for label, index, enabled in (('participant-a', 4, False), ('participant-b', 6, True)):
        n = p['genesis']['neurons'][index]
        write(label + ('-enable' if enabled else '-disable') + '-auto-stake',
            'variant { Configure = record { operation = opt variant { ChangeAutoStakeMaturity = record { requested_setting_for_auto_stake_maturity = ' + str(enabled).lower() + ' } } } }',
            sub=staking_subaccount(n['controller'], n['memo']))
    did = acceptance.REPO / 'canisters/sns-rewards/jupiter_sns_rewards.did'
    shape = '(opt record { reward_sns_root_canister_id : opt opt principal; programme : opt ProgrammeConfig })'
    stages = [('recognition', policy)]
    patron = copy.deepcopy(policy)
    patron.update(version=2, patron_enabled=True,
        launch_timestamp_seconds=p['_acceptance_start_timestamp_seconds'])
    stages.append(('patron', patron))
    # Treasury is separate: enabling this stage authorizes a bounded 1-ICP
    # operation, subject to balance, native framework limits and seven-day gate.
    treasury = copy.deepcopy(patron)
    treasury['version'] = 3
    treasury['automation'].update(treasury_enabled=True, treasury_retain_icp_e8s=100_000_000,
        treasury_max_proposal_icp_e8s=100_000_000)
    stages.append(('treasury', treasury))
    continuation = copy.deepcopy(treasury)
    continuation['version'] = 4
    stages.append(('treasury-continuation', continuation))
    for name, value in stages:
        text = 'opt ' + cd.record(reward_sns_root_canister_id='null',
            programme='opt ' + prepare.programme_arguments(acceptance.validate({k:v for k,v in p.items() if not k.startswith('_')}), value))
        cd.write_constructor(out, name + '-upgrade-args', did, shape, text)
        (out / (name + '-policy.json')).write_text(json.dumps(value, indent=2) + '\n')
    # Actual upgrade proposals are generated only when exact candidate Wasm is
    # supplied to the upgrade subcommand; never emit a fake executable payload.


def candidate(profile, decisions, artifacts, out, build_wasms=False):
    p, ids, policy = render(profile, decisions)
    transition = decisions['bootstrap_disburser']
    if (not isinstance(transition, dict) or set(transition) != {'transition', 'state_and_pending_effects', 'source_record'}
            or transition['transition'] not in ('preserving-upgrade', 'disposable-helper-replacement')
            or any(not isinstance(transition[key], str) or not transition[key].strip()
                   for key in ('state_and_pending_effects', 'source_record'))):
        raise ValueError('bootstrap purpose, state/pending effects and supported transition are unresolved')
    out = Path(out)
    if out.exists():
        raise ValueError('refusing to overwrite candidate')
    out.mkdir(parents=True)
    (out / 'profile.json').write_text(json.dumps(p, indent=2) + '\n')
    (out / 'operator-inputs.json').write_text(json.dumps(decisions, indent=2) + '\n')
    (out / 'neuron-ids.json').write_text(json.dumps(ids, indent=2) + '\n')
    prepare.applications(p, out / 'applications')  # programme absent initially
    prepare.framework(p, artifacts, out / 'sns', decisions['genesis_timestamp_seconds'])
    for group in ('applications', 'sns'):
        directory = out / group
        hashes = {path.name: hashlib.sha256(path.read_bytes()).hexdigest()
                  for path in directory.iterdir() if path.is_file()}
        (directory / 'sha256.json').write_text(json.dumps(hashes, sort_keys=True, indent=2) + '\n')
    official = out / 'framework-artifacts'
    official.mkdir()
    for role in ('governance', 'root', 'ledger', 'index', 'swap', 'archive'):
        (official / (role + '.wasm.gz')).write_bytes(Path(artifacts, role + '.wasm.gz').read_bytes())
    recipes(dict(p, _acceptance_start_timestamp_seconds=decisions['acceptance_start_timestamp_seconds']),
        ids, policy, out / 'sns/governance.interface.did', out / 'governance')
    if build_wasms:
        # Existing component builder owns acceptance bindings, embedded Relay,
        # frontend rendering and production/debug separation. No new installer.
        import build
        for role in acceptance.APPLICATIONS:
            if role != 'jupiter_relay':  # Historian builds/embeds this exact Relay.
                build.build(role, out / 'profile.json', out / 'artifacts')
        for stage in ('recognition', 'patron', 'treasury', 'treasury-continuation'):
            action = upgrade_action(p['roles']['jupiter_sns_rewards'],
                (out / 'artifacts/jupiter_sns_rewards.wasm.gz').read_bytes(),
                (out / ('governance/' + stage + '-upgrade-args.bin')).read_bytes())
            cd.write_constructor(out / 'governance', stage + '-proposal',
                out / 'sns/governance.interface.did', '(ManageNeuron)',
                manage_value(ids['governance'], proposal(action, 'Acceptance ' + stage)))
    manifest(out)


def manifest(out):
    out = Path(out)
    records = {str(p.relative_to(out)): hashlib.sha256(p.read_bytes()).hexdigest()
               for p in sorted(out.rglob('*')) if p.is_file() and p.name != 'manifest.json'}
    source = {key: subprocess.check_output(command, cwd=acceptance.REPO, text=True).strip()
        for key, command in dict(head=['git', 'rev-parse', 'HEAD'], tree=['git', 'rev-parse', 'HEAD^{tree}'],
            dirty=['git', 'status', '--short'], didc=['didc', '--version'],
            ic_wasm=['ic-wasm', '--version'], rust=['rustc', '--version']).items()}
    source['tracked_diff_sha256'] = hashlib.sha256(subprocess.check_output(
        ['git', 'diff', 'HEAD', '--binary'], cwd=acceptance.REPO)).hexdigest()
    source['preparation_sources_sha256'] = {str(p.relative_to(acceptance.REPO)): hashlib.sha256(p.read_bytes()).hexdigest()
        for p in sorted((acceptance.REPO / 'tools/acceptance').glob('*.py'))}
    (out / 'manifest.json').write_text(json.dumps(dict(canonical_production_release=False,
        mainnet_operations_authorized=False, live_observations_included=False,
        source=source, files=records), indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    make = commands.add_parser('candidate')
    for name in ('profile', 'decisions', 'artifacts', 'output'):
        make.add_argument(name)
    make.add_argument('--build', action='store_true', help='also build all seven non-debug acceptance applications')
    upgrade = commands.add_parser('upgrade')
    for name in ('governance_did', 'neuron_id_hex', 'canister_id', 'wasm', 'upgrade_args', 'output'):
        upgrade.add_argument(name)
    vote = commands.add_parser('vote')
    for name in ('governance_did', 'neuron_id_hex', 'proposal_id', 'output'):
        vote.add_argument(name)
    permissions = commands.add_parser('permissions')
    for name in ('governance_did', 'neuron_id_hex', 'principal', 'operation', 'output'):
        permissions.add_argument(name)
    args = parser.parse_args()
    try:
        if args.command == 'candidate':
            candidate(json.loads(Path(args.profile).read_text()), json.loads(Path(args.decisions).read_text()), args.artifacts, args.output, args.build)
        else:
            out = Path(args.output)
            out.mkdir()
            if args.command == 'upgrade':
                action = upgrade_action(args.canister_id, Path(args.wasm).read_bytes(), Path(args.upgrade_args).read_bytes())
                command = proposal(action, 'Reviewed acceptance application upgrade')
            elif args.command == 'vote':
                proposal_id = natural(int(args.proposal_id), 'proposal ID', positive=True)
                command = 'variant { RegisterVote = ' + cd.record(proposal='opt ' + cd.record(id=str(proposal_id)), vote='1 : int32') + ' }'
            else:
                acceptance.isolated_principal(args.principal)
                if args.operation not in ('grant', 'revoke'):
                    raise ValueError('permissions operation must be grant or revoke')
                name, field = ('AddNeuronPermissions', 'permissions_to_add') if args.operation == 'grant' else ('RemoveNeuronPermissions', 'permissions_to_remove')
                command = 'variant { ' + name + ' = ' + cd.record(principal_id='opt ' + cd.principal(args.principal),
                    **{field: 'opt record { permissions = vec { 2 : int32; 3; 4 } }'}) + ' }'
            cd.write_constructor(out, 'proposal', args.governance_did, '(ManageNeuron)', manage_value(args.neuron_id_hex, command))
            manifest(out)
    except (ValueError, KeyError, OSError) as error:
        parser.exit(1, f'operational preparation rejected: {error}\n')
