"""Off-chain official SNS mechanics. No Jupiter/IO economics or network operations.

Project adapters supply identities, genesis and controller policy. The six-part
reviewed lock includes the archive template, not a sixth initial canister.
"""
import gzip
import hashlib
import json
from pathlib import Path
import subprocess

import candid as cd
from ic import principal_bytes, staking_subaccount


def sha(data):
    return hashlib.sha256(data).hexdigest()


def verify_artifacts(lock, directory):
    directory = Path(directory)
    result = {}
    for role, pin in lock['components'].items():
        raw = (directory / pin['file']).read_bytes()
        compressed = (directory / (role + '.wasm.gz')).read_bytes()
        if sha(raw) != pin['raw_sha256'] or sha(compressed) != pin['gzip_sha256']:
            raise ValueError(f'{role}: raw/compressed artifact hash mismatch')
        if gzip.decompress(compressed) != raw:
            raise ValueError(f'{role}: compressed payload differs')
        # ic-wasm prints one extra newline after the exact custom-section value.
        did = subprocess.check_output(['ic-wasm', str(directory / pin['file']),
                                       'metadata', 'candid:service'])[:-1]
        revision = subprocess.check_output(['ic-wasm', str(directory / pin['file']),
                                            'metadata', 'git_commit_id']).decode().strip()
        if sha(did) != pin['did_sha256'] or revision != pin['revision']:
            raise ValueError(f'{role}: interface/source revision mismatch')
        result[role] = did
    if set(result) != {'governance', 'root', 'ledger', 'index', 'swap', 'archive'}:
        raise ValueError('exactly six reviewed SNS artifacts required')
    return result


def constructors(lock, interfaces, identities, genesis, timestamp, directory):
    """Serialize against the exact official types; never invent sale proceeds.

    `parameters` is a project-reviewed NervousSystemParameters Candid record.
    There are no token/economic defaults here. The local fixture is explicitly
    labelled and cannot serve as the unresolved Jupiter mainnet genesis.
    """
    out = Path(directory)
    out.mkdir()  # refuse to overwrite earlier evidence
    required = {'purpose', 'token_name', 'token_symbol', 'decimals', 'fee',
                'parameters', 'neurons', 'balances', 'total_supply',
                'archive_cycles', 'minimum_neuron_stake', 'metadata'}
    if set(genesis) != required:
        raise ValueError('missing/unexpected project genesis inputs')
    if genesis['purpose'] not in ('local-fixture-not-for-deployment', 'reviewed-acceptance'):
        raise ValueError('genesis purpose must be explicit')
    root, gov, ledger, index, swap = (identities[key] for key in
                                     ('root', 'governance', 'ledger', 'index', 'swap'))
    principals = [root, gov, ledger, index, swap, *identities['dapps']]
    if len(set(principals)) != len(principals):
        raise ValueError('framework and application identities must be distinct')
    for value in principals + [identities['operator']]:
        if not principal_bytes(value) or value == '2vxsx-fae':
            raise ValueError('non-anonymous framework/application identities required')
    for role, did in interfaces.items():
        (out / (role + '.interface.did')).write_bytes(did)
    # Check the common fields through the official type checker rather than
    # maintaining a Candid parser or allowing Swap/Ledger fees to contradict Gov.
    policy_type = '(record { transaction_fee_e8s : opt nat64; neuron_minimum_stake_e8s : opt nat64 })'
    expected_policy = cd.record(transaction_fee_e8s='opt ' + str(genesis['fee']),
                                neuron_minimum_stake_e8s='opt ' + str(genesis['minimum_neuron_stake']))
    did = out / 'governance.interface.did'
    if cd.encode_checked(did, policy_type, '(' + genesis['parameters'] + ')') != cd.encode_checked(did, policy_type, '(' + expected_policy + ')'):
        raise ValueError('Governance/Ledger/Swap fee or minimum stake mismatch')
    version = cd.record(**{role + '_wasm_hash': cd.blob(bytes.fromhex(pin['raw_sha256']))
                          for role, pin in lock['components'].items()})
    balances = list(genesis['balances'])
    neurons = []
    neuron_ids = set()
    for n in genesis['neurons']:
        if type(n['voting_power_percentage_multiplier']) is not int or not 0 <= n['voting_power_percentage_multiplier'] <= 100:
            raise ValueError('explicit voting-power multiplier required in [0, 100]')
        if type(n['auto_stake_maturity']) is not bool:
            raise ValueError('explicit auto-stake maturity policy required')
        vesting = n['vesting_period_seconds']
        if vesting is not None and (type(vesting) is not int or not 0 <= vesting < 2**64):
            raise ValueError('explicit valid vesting period required')
        sub = staking_subaccount(n['controller'], n['memo'])
        if sub in neuron_ids:
            raise ValueError('duplicate genesis neuron')
        neuron_ids.add(sub)
        balances.append({'owner': gov, 'subaccountHex': sub, 'amount': n['stake']})
        permissions = cd.vector(cd.record(principal='opt ' + cd.principal(p['principal']),
            permission_type=cd.vector(str(value) + ': int32' for value in p['permissions']))
            for p in n['permissions'])
        neuron = cd.record(
            id='opt ' + cd.record(id=cd.blob(bytes.fromhex(sub))), permissions=permissions,
            cached_neuron_stake_e8s=str(n['stake']), neuron_fees_e8s='0',
            created_timestamp_seconds=str(timestamp), aging_since_timestamp_seconds=str(timestamp),
            maturity_e8s_equivalent='0', voting_power_percentage_multiplier=str(n['voting_power_percentage_multiplier']),
            dissolve_state='opt variant { DissolveDelaySeconds = ' + str(n['dissolve_delay']) + ' }',
            auto_stake_maturity='opt ' + str(n['auto_stake_maturity']).lower(),
            vesting_period_seconds='null' if vesting is None else 'opt ' + str(vesting),
            followees='vec {}', disburse_maturity_in_progress='vec {}')
        neurons.append('record { ' + cd.text(sub) + '; ' + neuron + ' }')
    if any(type(b['amount']) is not int or b['amount'] < 0 for b in balances):
        raise ValueError('nonnegative exact integer token balances required')
    if sum(b['amount'] for b in balances) != genesis['total_supply']:
        raise ValueError('genesis supply does not reconcile')
    if type(genesis['archive_cycles']) is not int or genesis['archive_cycles'] <= 0:
        raise ValueError('explicit archive-creation cycles funding required')
    metadata = cd.record(**{key: 'opt ' + cd.text(value)
                           for key, value in genesis['metadata'].items()})
    governance = cd.record(root_canister_id='opt ' + cd.principal(root),
        ledger_canister_id='opt ' + cd.principal(ledger), swap_canister_id='opt ' + cd.principal(swap),
        mode='1 : int32', parameters='opt ' + genesis['parameters'],
        deployed_version='opt ' + version, sns_initialization_parameters=cd.text(json.dumps(genesis)),
        proposals='vec {}', in_flight_commands='vec {}', id_to_nervous_system_functions='vec {}',
        neurons=cd.vector(neurons), genesis_timestamp_seconds=str(timestamp), sns_metadata='opt ' + metadata)
    root_arg = cd.record(governance_canister_id='opt ' + cd.principal(gov),
        ledger_canister_id='opt ' + cd.principal(ledger), swap_canister_id='opt ' + cd.principal(swap),
        index_canister_id='opt ' + cd.principal(index),
        dapp_canister_ids=cd.vector(cd.principal(p) for p in identities['dapps']),
        archive_canister_ids='vec {}', testflight='false')
    archive = cd.record(trigger_threshold='2000', num_blocks_to_archive='1000',
        node_max_memory_size_bytes='opt 1073741824', max_message_size_bytes='opt 131072',
        controller_id=cd.principal(root), cycles_for_archive_creation='opt ' + str(genesis['archive_cycles']))
    ledger_arg = 'variant { Init = ' + cd.record(minting_account=cd.account({'owner': gov}),
        transfer_fee=str(genesis['fee']) + ' : nat', decimals='opt ' + str(genesis['decimals']),
        token_symbol=cd.text(genesis['token_symbol']), token_name=cd.text(genesis['token_name']),
        metadata='vec {}', initial_balances=cd.vector(
            'record { ' + cd.account(b) + '; ' + str(b['amount']) + ' : nat }' for b in balances),
        archive_options=archive, index_principal='opt ' + cd.principal(index)) + ' }'
    # Same reviewed parked Swap convention as IO. These are constructor
    # compatibility values, never sale proceeds or a valuation oracle.
    swap_arg = cd.record(sns_root_canister_id=cd.text(root), sns_governance_canister_id=cd.text(gov),
        sns_ledger_canister_id=cd.text(ledger),
        nns_governance_canister_id=cd.text(identities['nns_governance']),
        icp_ledger_canister_id=cd.text(identities['icp_ledger']),
        fallback_controller_principal_ids=cd.vector([cd.text(identities['operator'])]),
        sns_token_e8s='opt 0', should_auto_finalize='opt false',
        swap_start_timestamp_seconds='opt 18446744073709551615',
        swap_due_timestamp_seconds='opt 18446744073709551615', nns_proposal_id='opt 0',
        min_participants='opt 1', min_direct_participation_icp_e8s='opt 100000000',
        max_direct_participation_icp_e8s='opt 100000000',
        min_participant_icp_e8s='opt 100000000', max_participant_icp_e8s='opt 100000000',
        neurons_fund_participation='opt false', transaction_fee_e8s='opt ' + str(genesis['fee']),
        neuron_minimum_stake_e8s='opt ' + str(genesis['minimum_neuron_stake']),
        neuron_basket_construction_parameters='opt ' + cd.record(count='2', dissolve_delay_interval_seconds='86400'))
    index_arg = 'opt variant { Init = ' + cd.record(ledger_id=cd.principal(ledger)) + ' }'
    types = {'governance': 'Governance', 'root': 'SnsRootCanister', 'ledger': 'LedgerArg',
             'index': 'opt IndexArg', 'swap': 'Init'}
    for role, value in {'governance': governance, 'root': root_arg, 'ledger': ledger_arg,
                        'index': index_arg, 'swap': swap_arg}.items():
        cd.write_constructor(out, role, out / (role + '.interface.did'), '(' + types[role] + ')', value)
    (out / 'inputs.json').write_text(json.dumps({'genesis': genesis, 'identities': identities,
        'timestamp': timestamp, 'framework': lock}, sort_keys=True, indent=2) + '\n')
