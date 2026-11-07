"""One reviewed economic input for launch, operations and test fixtures."""
import json
from pathlib import Path

PATH = Path(__file__).resolve().parents[2] / 'deploy/acceptance/economics.json'


def load():
    value = json.loads(PATH.read_text())
    allocations = ('governance_stake_e8s', 'sale_allocation_e8s',
                   'proposer_stake_e8s', 'reserve_transport_e8s', 'builtin_treasury_e8s')
    if value['decimals'] != 8 or sum(value[key] for key in allocations) != value['initial_supply_e8s']:
        raise ValueError('jUP decimals/supply conservation failed')
    if value['halving_days'] * 86400 != value['year_seconds'] * 4:
        raise ValueError('year and halving interval disagree')
    return value


def launch_candidate(governance_fixture, reserve_controller, proposer_controller, dapp, logo):
    """Official SNS CLI v2 input. All principals here are labelled local fixtures.

    Does not create/install canisters or select a production NNX principal.
    The basket is 0/one-year; only its one-year member meets Jupiter bonus policy.
    Equal framework minimum/maximum delay is validated by the official CLI;
    Jupiter eligibility independently requires exactly one non-dissolving year.
    """
    e = load()
    tokens = lambda n: str(n) + ' e8s'
    seconds = lambda n: str(n) + ' seconds'
    return {
        'name': 'Jupiter', 'description': 'Jupiter local production-sale rehearsal.',
        'logo': str(logo), 'url': 'https://example.com/',
        'NnsProposal': {'title': 'Local Jupiter launch validation',
                        'url': 'https://example.com/', 'summary': 'Local fixture identities only.'},
        'fallback_controller_principals': [governance_fixture], 'dapp_canisters': [dapp],
        'Token': {'name': e['token_name'], 'symbol': e['token_symbol'],
                  'transaction_fee': tokens(e['transfer_fee_e8s']), 'logo': str(logo)},
        'Proposals': {'rejection_fee': tokens(e['proposal_rejection_e8s']),
                      'initial_voting_period': seconds(e['jupiter_voting_seconds']),
                      'maximum_wait_for_quiet_deadline_extension': '1 day'},
        'Neurons': {'minimum_creation_stake': '1 token'},
        'Voting': {'minimum_dissolve_delay': seconds(e['year_seconds']),
                   'MaximumVotingPowerBonuses': {
                       'DissolveDelay': {'duration': seconds(e['year_seconds']), 'bonus': '0%'},
                       'Age': {'duration': seconds(e['year_seconds'] * 4), 'bonus': '0%'}},
                   'RewardRate': {'initial': '0%', 'final': '0%', 'transition_duration': '0 seconds'}},
        'Distribution': {'Neurons': [
            {'principal': controller, 'memo': memo, 'stake': tokens(e[key]),
             'dissolve_delay': seconds(delay), 'vesting_period': '0 seconds'}
            for controller, memo, key, delay in [
                (governance_fixture, 1, 'governance_stake_e8s', e['year_seconds']),
                (reserve_controller, 2, 'reserve_transport_e8s', 0),
                (proposer_controller, 3, 'proposer_stake_e8s', e['year_seconds'])]],
            'InitialBalances': {'treasury': tokens(0), 'swap': tokens(e['sale_allocation_e8s'])},
            'total': tokens(e['initial_supply_e8s'])},
        'Swap': {'minimum_participants': 1,
                 'minimum_direct_participation_icp': tokens(e['sale_min_icp_e8s']),
                 'maximum_direct_participation_icp': tokens(e['sale_max_icp_e8s']),
                 'minimum_participant_icp': tokens(e['sale_min_participant_icp_e8s']),
                 'maximum_participant_icp': tokens(e['sale_max_icp_e8s']),
                 'VestingSchedule': {'events': 2, 'interval': seconds(e['year_seconds'])},
                 'start_time': None, 'duration': '7 days', 'neurons_fund_participation': False}}
