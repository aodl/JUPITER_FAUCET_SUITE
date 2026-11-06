"""Offline Jupiter acceptance bindings. No network, identity or deployment calls.

The checked profile deliberately has unresolved identities/economics. Local
fixtures supply owned PocketIC IDs; they never update the mainnet profile.
"""
import json
import re
from pathlib import Path
import sys

from ic import account, principal_bytes, staking_subaccount

REPO = Path(__file__).resolve().parents[2]
PROFILE = REPO / 'deploy/acceptance/profile.json'
APPLICATIONS = (
    'jupiter_disburser', 'jupiter_faucet', 'jupiter_lifeline',
    'jupiter_sns_rewards', 'jupiter_historian', 'jupiter_relay',
    'jupiter_faucet_frontend',
)
SNS = ('sns_root', 'sns_governance', 'sns_ledger', 'sns_index', 'sns_swap')
# IO source 8b2e1cba, deploy/acceptance/acceptance-manifest.json. No sibling dependency.
PROTECTED = (
    'uccpi-cqaaa-aaaar-qby3q-cai', 'acjuz-liaaa-aaaar-qb4qq-cai',
    'afisn-gqaaa-aaaar-qb4qa-cai', 'alk7f-5aaaa-aaaar-qb4ra-cai',
    'j5gs6-uiaaa-aaaar-qb5cq-cai', 'u2qkp-aqaaa-aaaar-qb7ea-cai',
    'jufzc-caaaa-aaaar-qb5da-cai', 'thset-pqaaa-aaaar-qb7wa-cai',
    'oae4c-3iaaa-aaaar-qb5qq-cai', 'tjqj3-uaaaa-aaaar-qb7xa-cai',
    'torpp-zyaaa-aaaar-qb7xq-cai',
    # Retain third-party compatibility tests, but never configure this former
    # development Root as an acceptance framework/application identity.
    '3e3x2-xyaaa-aaaaq-aaalq-cai',
    '2jvtu-yqaaa-aaaaq-aaama-cai', '2ouva-viaaa-aaaaq-aaamq-cai',
)
PROTECTED_NEURONS = (11614578985374291210, 10292412127977304661)
PROTECTED_STAKING = (
    'ff0c0b36afefffd0c7a4d85c0bcea366acd6d74f45f7703d0783cc6448899c68',
)
# Exact shared external beneficiary in Disburser's reviewed 5% age-bonus route.
# This is neither the Jupiter staking Account nor IO's protected permanent neuron.
DQUORUM_STAKING_SUBACCOUNT = '77e63de72b5e3339ea20f4baf3ec2bd92138ddde0daeb69db50acceb384bdf0f'
SYSTEM = {
    'ledger': 'ryjl3-tyaaa-aaaaa-aaaba-cai',
    'index': 'qhbym-qaaaa-aaaaa-aaafq-cai',
    'governance': 'rrkah-fqaaa-aaaaa-aaaaq-cai',
    'cmc': 'rkp4c-7iaaa-aaaaa-aaaca-cai',
    'sns_wasm': 'qaa6y-5yaaa-aaaaa-aaafa-cai',
    'xrc': 'uf6dk-hyaaa-aaaaq-qaaaq-cai',
}


def isolated_principal(text):
    raw = principal_bytes(text)
    if not raw or raw == b'\x04' or text in PROTECTED or text in SYSTEM.values():
        raise ValueError(f'protected/system/anonymous application principal: {text}')
    return text



def validate(profile):
    if set(profile) != {'environment', 'network', 'operator', 'roles', 'nns_neuron',
                        'age_bonus_recipient_2', 'relay_surplus_neuron_recipients',
                        'initial_main_cycles_target', 'genesis'}:
        raise ValueError('unexpected/missing profile fields')
    if profile['environment'] != 'acceptance' or profile['network'] != 'ic':
        raise ValueError('acceptance must explicitly target the real ic network')
    operator = isolated_principal(profile['operator'])
    roles = profile['roles']
    if set(roles) != set(APPLICATIONS + SNS):
        raise ValueError('exactly seven application and five initial SNS roles required')
    for role, value in roles.items():
        isolated_principal(value)
        if value == operator:
            raise ValueError(f'{role} cannot be the operator')
    if len(set(roles.values())) != len(roles):
        raise ValueError('application/SNS identities must be distinct')
    neuron = profile['nns_neuron']
    if set(neuron) != {'id', 'memo'}:
        raise ValueError('neuron requires id and staking memo')
    for key in ('id', 'memo'):
        value = neuron[key]
        if type(value) is not int or not 0 <= value < 2**64:
            raise ValueError(f'unresolved/invalid NNS neuron {key}')
    if not neuron['id'] or neuron['id'] in PROTECTED_NEURONS:
        raise ValueError('acceptance NNS neuron must be distinct from production')
    if neuron['memo'] != 0:
        raise ValueError('Jupiter staking/wakeup convention requires the existing nonce zero')
    if profile['initial_main_cycles_target'] != 3_000_000_000_000:
        raise ValueError('reviewed initial main-cycles target is 3T; archive funding is separate')
    sub = staking_subaccount(roles['jupiter_disburser'], neuron['memo'])
    if sub in PROTECTED_STAKING:
        raise ValueError('production staking subaccount')
    recipient = profile['age_bonus_recipient_2']
    if recipient is not None:
        if set(recipient) != {'owner', 'subaccountHex'}:
            raise ValueError('age-bonus account requires owner and subaccountHex')
        owner = recipient['owner']
        if owner != SYSTEM['governance'] and owner not in {operator, *roles.values()}:
            raise ValueError('financial recipient is outside the explicit environment allowlist')
        derived = account(owner, recipient['subaccountHex'])
        # Only this explicitly selected third-party Account is reviewed. All
        # other Governance Accounts stay closed, including protected Jupiter/IO
        # staking Accounts; a neuron ID does not establish its staking bytes.
        if owner == SYSTEM['governance'] and derived['subaccountHex'] != DQUORUM_STAKING_SUBACCOUNT:
            raise ValueError('production/unreviewed Governance financial destination')
        recipient = derived
    surplus = profile['relay_surplus_neuron_recipients']
    if surplus is not None:
        if not isinstance(surplus, list) or len(surplus) > 5:
            raise ValueError('explicit zero-to-five Relay recipients required')
        seen = set()
        for entry in surplus:
            if set(entry) != {'neuron_id', 'memo_hex'}:
                raise ValueError('Relay recipient requires neuron_id and memo_hex')
            neuron_id = entry['neuron_id']
            if type(neuron_id) is not int or not 0 < neuron_id < 2**64 or neuron_id in PROTECTED_NEURONS:
                raise ValueError('protected/invalid Relay surplus neuron')
            if neuron_id in seen or len(bytes.fromhex(entry['memo_hex'])) > 32:
                raise ValueError('duplicate Relay recipient or oversized memo')
            seen.add(neuron_id)
    if profile['genesis'] is not None:
        genesis = profile['genesis']
        for balance in genesis['balances']:
            isolated_principal(balance['owner'])
            account(balance['owner'], balance.get('subaccountHex'))
        for sns_neuron in genesis['neurons']:
            isolated_principal(sns_neuron['controller'])
            for permission in sns_neuron['permissions']:
                isolated_principal(permission['principal'])
    return {
        'environment': 'acceptance', 'network': 'ic', 'roles': roles,
        'operator': operator, 'neuronId': str(neuron['id']),
        'stakingAccount': account(SYSTEM['governance'], sub),
        'ageBonusRecipient2': recipient,
    }


def bindings(profile):
    checked = validate(profile)
    return {
        **{'JUPITER_BOUND_' + role.removeprefix('jupiter_').upper() + '_ID': value
           for role, value in checked['roles'].items()},
        'JUPITER_BOUND_FAUCET_NEURON_ID': checked['neuronId'],
        'JUPITER_CONFIGURED_SNS_ROOT': checked['roles']['sns_root'],
        'JUPITER_BOUND_APPLICATION_CANISTERS': ','.join(checked['roles'][role] for role in APPLICATIONS),
        'JUPITER_BOUND_MANAGED_CANISTERS': ','.join(value for role, value in checked['roles'].items() if role != 'jupiter_relay'),
    }


def frontend(profile):
    checked = validate(profile)
    return {
        'network': 'acceptance',
        'historianCanisterId': checked['roles']['jupiter_historian'],
        'frontendCanisterId': checked['roles']['jupiter_faucet_frontend'],
        'ledgerCanisterId': SYSTEM['ledger'],
        'relayCanisterId': checked['roles']['jupiter_relay'],
        'neuronId': checked['neuronId'],
        'stakingAccount': checked['stakingAccount'],
        # No acceptance IO destination is enabled before the cross-project contract.
        'ioStakingSubaccountHex': None,
    }


def render_markup(profile, text):
    checked = validate(profile)
    production = json.loads((REPO / '.icp/data/mappings/ic.ids.json').read_text())
    replacements = {}
    old_staking = account(SYSTEM['governance'], PROTECTED_STAKING[0])
    new_staking = checked['stakingAccount']
    for key in ('address', 'explorerAccountHex', 'subaccountHex'):
        replacements[old_staking[key]] = new_staking[key]
    replacements[str(PROTECTED_NEURONS[0])] = checked['neuronId']
    for role in APPLICATIONS:
        old, new = production[role], checked['roles'][role]
        replacements[old] = new
        replacements[account(old)['explorerAccountHex']] = account(new)['explorerAccountHex']
    # IO has no accepted cross-project routing/settlement contract yet. Do not
    # leave a clickable production IO neuron/account in an acceptance page.
    # Keep the frontend's existing unconfigured IO/D-QUORUM links disabled;
    # approving an explicit Disburser beneficiary does not activate IO flows.
    io_account = account(SYSTEM['governance'], DQUORUM_STAKING_SUBACCOUNT)
    for destination in (str(PROTECTED_NEURONS[1]), io_account['explorerAccountHex']):
        text = re.sub(r'<a\b[^>]*href="[^"]*' + re.escape(destination)
                      + r'[^"]*"[^>]*>(.*?)</a>',
                      r'<span title="IO acceptance integration is not configured">\1</span>',
                      text, flags=re.DOTALL)
    for old in sorted(replacements, key=len, reverse=True):
        text = text.replace(old, replacements[old])
    return text


if __name__ == '__main__':
    try:
        command = sys.argv[1]
        source = Path(sys.argv[2]) if len(sys.argv) > 2 else PROFILE
        profile = json.loads(source.read_text())
        if command == 'render':
            print(render_markup(profile, sys.stdin.read()), end='')
        else:
            result = {'bindings': bindings, 'frontend': frontend, 'check': validate}[command](profile)
            print(json.dumps(result, sort_keys=True))
    except (ValueError, KeyError, OSError, IndexError) as error:
        sys.exit(f'acceptance profile rejected: {error}')
