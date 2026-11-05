#!/usr/bin/env python3
"""Offline Jupiter adapter. Writes constructor inputs; cannot install anything."""
import hashlib
import json
from pathlib import Path
import sys

import candid as cd
import profile as acceptance
import sns


def application_arguments(profile):
    checked = acceptance.validate(profile)
    roles = checked['roles']
    if checked['ageBonusRecipient2'] is None or profile['relay_surplus_neuron_recipients'] is None:
        raise ValueError('age-bonus Account and canonical Relay surplus policy require explicit decisions')
    system = acceptance.SYSTEM
    opt = lambda value: 'opt ' + cd.principal(value)
    default = lambda role: cd.account({'owner': roles[role]})
    targets = cd.vector(cd.principal(value) for role, value in roles.items() if role != 'jupiter_relay')
    args = {
        'jupiter_disburser': cd.record(neuron_id=checked['neuronId'] + ' : nat64',
            normal_recipient=default('jupiter_faucet'), age_bonus_recipient_1=default('jupiter_sns_rewards'),
            age_bonus_recipient_2=cd.account(checked['ageBonusRecipient2']),
            ledger_canister_id=opt(system['ledger']), governance_canister_id=opt(system['governance']),
            rescue_controller=cd.principal(roles['jupiter_lifeline']), autonomous_rescue_armed='opt false',
            main_interval_seconds='opt 86400', rescue_interval_seconds='opt 86400'),
        'jupiter_faucet': cd.record(staking_account=cd.account(checked['stakingAccount']),
            payout_subaccount='null', ledger_canister_id=opt(system['ledger']),
            index_canister_id=opt(system['index']), cmc_canister_id=opt(system['cmc']),
            governance_canister_id=opt(system['governance']),
            funding_source_account=default('jupiter_disburser'),
            rescue_controller=cd.principal(roles['jupiter_lifeline']), autonomous_rescue_armed='opt false',
            main_interval_seconds='opt 86400', rescue_interval_seconds='opt 86400',
            min_tx_e8s='opt 100000000', stake_recognition_delay_seconds='opt 604800'),
        'jupiter_historian': cd.record(staking_account=cd.account(checked['stakingAccount']),
            output_source_account='opt ' + default('jupiter_disburser'),
            output_account='opt ' + default('jupiter_faucet'), rewards_account='opt ' + default('jupiter_sns_rewards'),
            ledger_canister_id=opt(system['ledger']), index_canister_id=opt(system['index']),
            cmc_canister_id=opt(system['cmc']), faucet_canister_id=opt(roles['jupiter_faucet']),
            sns_wasm_canister_id=opt(system['sns_wasm']), xrc_canister_id=opt(system['xrc']),
            enable_sns_tracking='opt true', canonical_relay_canister_id=opt(roles['jupiter_relay']),
            canonical_relay_targets='opt ' + targets, relay_factory_enabled='opt true'),
        'jupiter_sns_rewards': cd.record(reward_sns_root_canister_id=opt(roles['sns_root'])),
        'jupiter_relay': cd.record(managed_canisters=targets,
            ledger_canister_id=opt(system['ledger']), cmc_canister_id=opt(system['cmc']),
            governance_canister_id=opt(system['governance']), blackhole_canister_id='null',
            sns_rewards_canister_id=opt(roles['jupiter_sns_rewards']), icp_index_canister_id=opt(system['index']),
            main_interval_seconds='opt 86400', max_transfers_per_tick='opt 50',
            surplus_canister_recipients='null',
            surplus_neuron_recipients=cd.vector(cd.record(neuron_id=str(n['neuron_id']) + ' : nat64',
                memo=cd.blob(bytes.fromhex(n['memo_hex']))) for n in profile['relay_surplus_neuron_recipients'])),
    }
    return checked, args


def applications(profile, out):
    checked, args = application_arguments(profile)
    out = Path(out)
    out.mkdir()
    for role, value in args.items():
        component = role.removeprefix('jupiter_').replace('_', '-')
        did = acceptance.REPO / 'canisters' / component / (role + '.did')
        cd.write_constructor(out, role, did, '(InitArgs)', value)
    for role in ('jupiter_lifeline', 'jupiter_faucet_frontend'):
        (out / (role + '.bin')).write_bytes(bytes.fromhex('4449444c0000'))
        (out / (role + '.did')).write_text('()\n')
    (out / 'frontend.json').write_text(json.dumps(acceptance.frontend(profile), sort_keys=True, indent=2) + '\n')
    (out / 'accounts.json').write_text(json.dumps(checked, sort_keys=True, indent=2) + '\n')
    (out / 'profile.json').write_text(json.dumps(profile, sort_keys=True, indent=2) + '\n')
    roles = checked['roles']
    plan = {role: [checked['operator']] for role in roles}
    final = {role: [roles['sns_root']] for role in roles}
    final['sns_root'] = [roles['sns_governance']]
    for role in ('jupiter_faucet', 'jupiter_disburser'):
        final[role] = [roles[role]]
    # Canonical Relay remains replacement-style; immutable factory children are
    # separately controllerless. Neither receives a hidden operator controller.
    (out / 'controller-plan.json').write_text(json.dumps({
        'bootstrap': plan, 'proposed_final': final,
        'arming_handoff': {role: [checked['operator'], roles[role]]
                           for role in ('jupiter_faucet', 'jupiter_disburser')},
        'autonomous_rescue': {role: [roles[role], roles['jupiter_lifeline']]
                              for role in ('jupiter_faucet', 'jupiter_disburser')},
        'factory_children': [], 'handoff_requires_operator_review': True,
        'lifeline_recovery_code': 'not implemented; incident-specific reviewed upgrade',
    }, indent=2) + '\n')


def framework(profile, artifact_directory, out, timestamp, local_fixture=False):
    checked = acceptance.validate(profile)
    genesis = profile['genesis']
    if genesis is None:
        raise ValueError('jUP genesis is unresolved; no SNS installation payloads generated')
    if genesis['purpose'] == 'local-fixture-not-for-deployment' and not local_fixture:
        raise ValueError('local economics cannot produce deployable acceptance payloads')
    roles = checked['roles']
    identities = {role: roles['sns_' + role] for role in ('root', 'governance', 'ledger', 'index', 'swap')}
    identities.update(operator=checked['operator'], icp_ledger=acceptance.SYSTEM['ledger'],
                      nns_governance=acceptance.SYSTEM['governance'],
                      dapps=[roles[role] for role in acceptance.APPLICATIONS
                             if role not in ('jupiter_disburser', 'jupiter_faucet')])
    lock = json.loads((acceptance.REPO / 'deploy/acceptance/sns-framework.json').read_text())
    interfaces = sns.verify_artifacts(lock, artifact_directory)
    sns.constructors(lock, interfaces, identities, genesis, timestamp, out)
    (Path(out) / 'profile.json').write_text(json.dumps(profile, sort_keys=True, indent=2) + '\n')


if __name__ == '__main__':
    try:
        command, source, *arguments = sys.argv[1:]
        profile = json.loads(Path(source).read_text())
        if command == 'applications' and len(arguments) == 1:
            applications(profile, arguments[0])
        elif command == 'sns' and len(arguments) in (3, 4):
            framework(profile, arguments[0], arguments[1], int(arguments[2]),
                      arguments[3:] == ['--local-fixture'])
        else:
            raise ValueError('usage: prepare.py applications PROFILE OUT | sns PROFILE ARTIFACTS OUT TIMESTAMP [--local-fixture]')
        directory = Path(arguments[0] if command == 'applications' else arguments[1])
        hashes = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                  for p in directory.iterdir() if p.is_file()}
        (directory / 'sha256.json').write_text(json.dumps(hashes, sort_keys=True, indent=2) + '\n')
    except (ValueError, KeyError, OSError, IndexError) as error:
        sys.exit(f'acceptance preparation rejected: {error}')
