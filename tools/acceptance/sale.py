"""Typed production SNS-W post-execution payload from the maintained economics.

The official CLI separately validates the NNS proposal conversion. The local
adapter supplies only fixture identities and the explicit NNS execution times;
SNS-W performs its authentic post-execution and canister-constructor validation.
"""
import json
from pathlib import Path
import sys

import candid as cd
import economics


def prepare(context, did, output):
    e = economics.load()
    opt = lambda n: 'opt ' + str(n)
    image = json.loads(Path('tests/pocketic/fixtures/jup-sns-genesis.json').read_text())['metadata']['logo']
    neurons = []
    for controller, memo, key, delay in [(context['nnx_fixture'], 1, 'governance_stake_e8s', e['year_seconds']),
            (context['rewards'], 2, 'reserve_transport_e8s', 0),
            (context['rewards'], 3, 'proposer_stake_e8s', e['year_seconds'])]:
        neurons.append(cd.record(controller='opt ' + cd.principal(controller), memo=str(memo),
            stake_e8s=str(e[key]), dissolve_delay_seconds=str(delay), vesting_period_seconds='opt 0'))
    distribution = cd.record(developer_distribution='opt ' + cd.record(developer_neurons=cd.vector(neurons)),
        treasury_distribution='opt ' + cd.record(total_e8s=str(e['builtin_treasury_e8s'])),
        swap_distribution='opt ' + cd.record(total_e8s=str(e['sale_allocation_e8s']),
            initial_swap_amount_e8s=str(e['sale_allocation_e8s'])))
    payload = cd.record(
        url='opt ' + cd.text('https://example.com/'), name='opt ' + cd.text('Jupiter'),
        description='opt ' + cd.text('Local production-allocation sale rehearsal; fixture NNX only.'),
        logo='opt ' + cd.text(image), token_logo='opt ' + cd.text(image),
        token_name='opt ' + cd.text(e['token_name']), token_symbol='opt ' + cd.text(e['token_symbol']),
        initial_token_distribution='opt variant { FractionalDeveloperVotingPower = ' + distribution + ' }',
        transaction_fee_e8s=opt(e['transfer_fee_e8s']), proposal_reject_cost_e8s=opt(e['proposal_rejection_e8s']),
        neuron_minimum_stake_e8s=opt(e['minimum_neuron_stake_e8s']),
        neuron_minimum_dissolve_delay_to_vote_seconds=opt(e['year_seconds']),
        max_dissolve_delay_seconds=opt(e['year_seconds']), max_dissolve_delay_bonus_percentage=opt(e['dissolve_bonus_percentage']),
        max_age_bonus_percentage=opt(e['age_bonus_percentage']), max_neuron_age_seconds_for_age_bonus=opt(e['year_seconds'] * 4),
        initial_reward_rate_basis_points=opt(e['native_initial_reward_basis_points']), final_reward_rate_basis_points=opt(e['native_final_reward_basis_points']),
        reward_rate_transition_duration_seconds='opt 0',
        initial_voting_period_seconds=opt(e['jupiter_voting_seconds']),
        wait_for_quiet_deadline_increase_seconds=opt(e['wait_for_quiet_seconds']),
        min_direct_participation_icp_e8s=opt(e['sale_min_icp_e8s']),
        max_direct_participation_icp_e8s=opt(e['sale_max_icp_e8s']),
        min_participant_icp_e8s=opt(e['sale_min_participant_icp_e8s']),
        max_participant_icp_e8s=opt(e['sale_max_icp_e8s']), min_participants='opt 1',
        neuron_basket_construction_parameters='opt ' + cd.record(count=str(e['sale_basket_count']), dissolve_delay_interval_seconds=str(e['year_seconds'])),
        fallback_controller_principal_ids=cd.vector([cd.text(context['fallback'])]),
        dapp_canisters='opt ' + cd.record(canisters=cd.vector([cd.record(id='opt ' + cd.principal(context['rewards']))])),
        nns_proposal_id=opt(context['nns_proposal_id']), swap_start_timestamp_seconds=opt(context['start']),
        swap_due_timestamp_seconds=opt(context['due']), neurons_fund_participation='opt false')
    output = Path(output)
    output.mkdir()
    cd.write_constructor(output, 'deploy', did, '(DeployNewSnsRequest)', cd.record(sns_init_payload='opt ' + payload))


if __name__ == '__main__':
    prepare(json.loads(Path(sys.argv[1]).read_text()), Path(sys.argv[2]), Path(sys.argv[3]))
