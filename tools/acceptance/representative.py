"""Small contested two-SNS fixture, with the maintained economic parameters.

Distribution/voting weights are intentionally representative, not the production
allocation or evidence of a sale. Only official SNS constructors are installed.
"""
import json
from pathlib import Path

import economics
import sns
from ic import staking_subaccount


def prepare(context, artifacts, output, target=False):
    e = economics.load()
    genesis = json.loads(Path('tests/pocketic/fixtures/jup-sns-genesis.json').read_text())
    genesis['fee'] = e['transfer_fee_e8s']
    genesis['minimum_neuron_stake'] = e['minimum_neuron_stake_e8s']
    period = e['target_voting_seconds'] if target else e['jupiter_voting_seconds']
    genesis['parameters'] = f'''record {{
      reject_cost_e8s = opt {e['proposal_rejection_e8s']};
      neuron_minimum_stake_e8s = opt {e['minimum_neuron_stake_e8s']};
      transaction_fee_e8s = opt {e['transfer_fee_e8s']};
      max_proposals_to_keep_per_action = opt 100;
      initial_voting_period_seconds = opt {period}; wait_for_quiet_deadline_increase_seconds = opt {e['wait_for_quiet_seconds']};
      default_followees = opt record {{ followees = vec {{}} }}; max_number_of_neurons = opt 1000;
      neuron_minimum_dissolve_delay_to_vote_seconds = opt {e['minimum_voting_dissolve_delay_seconds']};
      max_followees_per_function = opt 15; max_dissolve_delay_seconds = opt {e['max_dissolve_delay_seconds']};
      max_neuron_age_for_age_bonus = opt {e['max_neuron_age_for_age_bonus']}; max_number_of_proposals_with_ballots = opt 700;
      neuron_claimer_permissions = opt record {{ permissions = vec {{ 1;2;3;4;5;6;7;8;9;10 }} }};
      neuron_grantable_permissions = opt record {{ permissions = vec {{ 1;2;3;4;5;6;7;8;9;10 }} }};
      max_number_of_principals_per_neuron = opt 5;
      voting_rewards_parameters = opt record {{ initial_reward_rate_basis_points = opt 0;
       final_reward_rate_basis_points = opt 0; reward_rate_transition_duration_seconds = opt {e['native_reward_transition_seconds']}; round_duration_seconds = opt {e['native_reward_round_seconds']} }};
      max_dissolve_delay_bonus_percentage = opt {e['dissolve_bonus_percentage']}; max_age_bonus_percentage = opt {e['age_bonus_percentage']};
      maturity_modulation_disabled = opt true; automatically_advance_target_version = opt false;
    }}'''
    all_permissions = list(range(1, 11))
    definitions = [('a', context['a'], 1, 1000, all_permissions),
                   ('b', context['b'], 2, 1000, all_permissions)]
    if target:
        definitions += [('external', context['rewards'], 3, 1000, [4]),
                        ('io_only', context['io_only'], 4, 1000, all_permissions)]
    else:
        native = context.get('native_economics')
        governance_stake = e['governance_stake_e8s'] // 10**8 if native else 1000
        # Native exclusion tests give protocol neurons real economic authority:
        # absence of Disburse must not mask a missing exact-ID exclusion. These
        # are local representative stakes, not a production allocation change.
        definitions += [('nnx_fixture', context['nnx_fixture'], 1 if native else 3, governance_stake, all_permissions if native else [4]),
                        ('proposer', context['rewards'], 3 if native else 4, context['proposer_stake'], all_permissions if native else [3]),
                        ('a_second', context['a'], 5, 1, all_permissions)]
        if native:
            definitions += [('reserve_transport', context['rewards'], 2, 1, all_permissions)]
    genesis['neurons'] = [{'controller': controller, 'memo': memo, 'stake': stake * 10**8,
        'dissolve_delay': 0 if name == 'reserve_transport' else e['year_seconds'],
        'permissions': [{'principal': controller, 'permissions': permissions}],
        'voting_power_percentage_multiplier': 100, 'auto_stake_maturity': controller == context['a'],
        'vesting_period_seconds': 0} for name, controller, memo, stake, permissions in definitions]
    total = sum(n['stake'] for n in genesis['neurons'])
    # A direct dapp-held reserve is valid operational fixture data. It does not
    # prove the production launcher's reserve-transport disbursement.
    genesis['balances'] = [{'owner': context['rewards'], 'amount': e['initial_supply_e8s'] - total}]
    genesis['total_supply'] = e['initial_supply_e8s']
    identities = context['target' if target else 'jup'].copy()
    identities.update(operator=context['operator'], dapps=[context['app']] if target else [],
                      nns_governance=context['nns_governance'], icp_ledger=context['icp_ledger'])
    lock = json.loads(Path('deploy/acceptance/sns-framework.json').read_text())
    interfaces = sns.verify_artifacts(lock, artifacts)
    sns.constructors(lock, interfaces, identities, genesis, context['timestamp'], output)
    ids = {name: staking_subaccount(controller, memo) for name, controller, memo, _, _ in definitions}
    Path(output, 'neuron-ids.json').write_text(json.dumps(ids, indent=2) + '\n')


if __name__ == '__main__':
    import sys
    context = json.loads(Path(sys.argv[1]).read_text())
    prepare(context, sys.argv[2], Path(sys.argv[3]) / 'jup')
    prepare(context, sys.argv[2], Path(sys.argv[3]) / 'target', target=True)
