import unittest

import economics


class Economics(unittest.TestCase):
    def test_conservation_and_roles(self):
        e = economics.load()
        self.assertEqual(e['initial_supply_e8s'], 100_000_000 * 10**8)
        self.assertEqual(e['reserve_transport_e8s'], 89_989_900 * 10**8)
        self.assertEqual(e['governance_stake_e8s'], 10_000_000 * 10**8)
        self.assertEqual(e['sale_allocation_e8s'], 10_000 * 10**8)
        self.assertEqual(e['proposer_stake_e8s'], 100 * 10**8)
        self.assertEqual(e['builtin_treasury_e8s'], 0)
        self.assertEqual(e['native_initial_reward_basis_points'], 0)
        self.assertEqual(e['native_final_reward_basis_points'], 0)
        self.assertEqual(e['transfer_fee_e8s'], 10_000_000)
        self.assertEqual(e['proposal_rejection_e8s'], 100_000_000)
        self.assertEqual(e['halving_days'], 1461)
        self.assertEqual(e['year_seconds'] * 4, e['halving_days'] * 86400)
        self.assertNotEqual(e['proposer_stake_e8s'], e['governance_stake_e8s'])
        self.assertIsNone(e['launch_timestamp_seconds'])
        self.assertTrue(e['exclude_operational_proposer_from_external_rewards'])

    def test_launch_preserves_allocation_and_separates_roles(self):
        c = economics.launch_candidate('nnx-fixture', 'reserve', 'proposer', 'dapp', 'logo')
        self.assertEqual([n['principal'] for n in c['Distribution']['Neurons']],
                         ['nnx-fixture', 'reserve', 'proposer'])
        self.assertEqual(c['Distribution']['Neurons'][1]['dissolve_delay'], '0 seconds')
        self.assertEqual(c['Voting']['RewardRate']['initial'], '0%')
        self.assertEqual(c['Proposals']['initial_voting_period'], '432000 seconds')
