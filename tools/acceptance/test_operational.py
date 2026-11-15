import copy
import json
import os
from pathlib import Path
import tempfile
import unittest

import candid as cd
import economics
from ic import principal_text, staking_subaccount
import operational as op
import prepare
import profile as acceptance
from test_profile import fixture


def local_decisions():
    owner = lambda byte: principal_text(bytes([byte]) * 28 + b'\x02')
    e = economics.load()
    return dict(roles_confirmed=True, operator=fixture()['operator'],
        voting_authority=dict(principal=fixture()['operator'], kind='temporary-acceptance',
            executable_route='Owned PocketIC caller only; no live authority approval'),
        genesis_timestamp_seconds=1_792_000_000, acceptance_start_timestamp_seconds=1_792_000_000,
        test_allocations=[dict(owner=owner(byte), amount_e8s=5_000 * 10**8,
            kind='neuron', memo=memo, dissolve_delay_seconds=delay)
            for byte, memo, delay in [(82,10,0),(82,11,e['year_seconds']),
                                     (84,20,0),(84,21,e['year_seconds'])]],
        relay_surplus_neuron_recipients=[], bootstrap_disburser=None, approved=False)


class Operational(unittest.TestCase):
    def test_large_typed_constructor_round_trip_exceeds_one_os_argument(self):
        payload = bytes(range(256)) * 300
        encoded = cd.encode_checked(acceptance.REPO / 'canisters/sns-rewards/jupiter_sns_rewards.did',
            '(blob)', '(' + cd.blob(payload) + ')')
        self.assertGreater(len(encoded.hex()), 131072)

    def test_unresolved_operator_allows_only_fixed_executable_bindings(self):
        p = json.loads(acceptance.PROFILE.read_text())
        p['operator'] = None
        checked = acceptance.validate(p, require_operator=False)
        self.assertIsNone(checked['operator'])
        self.assertEqual(acceptance.bindings(p)['JUPITER_BOUND_RELAY_ID'], p['roles']['jupiter_relay'])
        self.assertEqual(acceptance.frontend(p)['stakingAccount'], checked['stakingAccount'])
        with self.assertRaisesRegex(ValueError, 'unresolved'):
            prepare.application_arguments(p)
        p['roles']['jupiter_relay'] = acceptance.PROTECTED[0]
        with self.assertRaisesRegex(ValueError, 'protected'):
            acceptance.bindings(p)

    def test_unresolved_inputs_emit_no_candidate_directory(self):
        decisions = json.loads((acceptance.PROFILE.parent / 'operator-inputs.json').read_text())
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / 'candidate'
            with self.assertRaisesRegex(ValueError, 'genesis_timestamp_seconds'):
                op.candidate(json.loads(acceptance.PROFILE.read_text()), decisions, '/unused', out)
            self.assertFalse(out.exists())

    def test_local_authority_is_never_implicit_live_approval(self):
        with self.assertRaisesRegex(ValueError, 'approval'):
            op.render(fixture(), local_decisions())

    def test_unknown_helper_cannot_emit_a_complete_installation_bundle(self):
        d = local_decisions()
        d['approved'] = True
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / 'candidate'
            with self.assertRaisesRegex(ValueError, 'bootstrap purpose'):
                op.candidate(fixture(), d, '/unused', out)
            self.assertFalse(out.exists())

    def test_exact_supply_native_permissions_and_exclusions(self):
        p, ids, policy = op.render(fixture(), local_decisions(), local=True)
        g = p['genesis']
        self.assertEqual(g['purpose'], 'local-fixture-not-for-deployment')
        self.assertEqual(g['total_supply'], 100_000_000 * 10**8)
        self.assertEqual(sum(n['stake'] for n in g['neurons']) + sum(b['amount'] for b in g['balances']), g['total_supply'])
        self.assertEqual([n['stake'] for n in g['neurons'][:3]], [10_000_000 * 10**8, 89_979_900 * 10**8, 100 * 10**8])
        self.assertEqual([n['permissions'][0]['permissions'] for n in g['neurons'][:3]], [[2, 3, 4], [5], [3]])
        self.assertEqual(g['neurons'][1]['vesting_period_seconds'], 0)
        self.assertEqual(g['neurons'][1]['dissolve_delay'], 0)
        self.assertEqual(set(policy['excluded_jup_neurons']), set(ids.values()))
        self.assertFalse(policy['automation']['treasury_enabled'])
        self.assertFalse(policy['patron_enabled'])
        self.assertEqual(policy['automation']['targets'], [])
        for name, n in zip(('governance', 'reserve_transport', 'proposer'), g['neurons']):
            self.assertEqual(ids[name], staking_subaccount(n['controller'], n['memo']))

    def test_test_bucket_cannot_mint_extra_or_overlap_protocol_roles(self):
        for mutate in (lambda d: d['test_allocations'][0].update(amount_e8s=10**16),
                       lambda d: d['test_allocations'][0].update(memo=999),
                       lambda d: d['test_allocations'][0].update(owner=fixture()['roles']['jupiter_sns_rewards']),
                       lambda d: [entry.update(owner=d['operator']) for entry in d['test_allocations'][:2]],
                       lambda d: [entry.update(owner=acceptance.PROTECTED_PERSONAL_PRINCIPAL) for entry in d['test_allocations'][:2]],
                       lambda d: d.update(acceptance_start_timestamp_seconds=None),
                       lambda d: d.update(relay_surplus_neuron_recipients=None),
                       lambda d: d['voting_authority'].update(principal=principal_text(bytes([81]) * 28 + b'\x02')),
                       lambda d: d['voting_authority'].update(executable_route='')):
            d = local_decisions()
            mutate(d)
            with self.assertRaises(ValueError):
                op.render(fixture(), d, local=True)

    def test_personal_principal_cannot_be_the_executor_or_temporary_voter(self):
        d = local_decisions()
        d['operator'] = acceptance.PROTECTED_PERSONAL_PRINCIPAL
        d['voting_authority']['principal'] = d['operator']
        with self.assertRaisesRegex(ValueError, 'protected'):
            op.render(fixture(), d, local=True)

    def test_current_authorities_have_exact_supply_and_only_schedule_is_unresolved(self):
        d = json.loads((acceptance.PROFILE.parent / 'operator-inputs.json').read_text())
        self.assertIsNone(d['genesis_timestamp_seconds'])
        self.assertIsNone(d['acceptance_start_timestamp_seconds'])
        # An in-memory local clock validates the actual public identities without
        # approving a mainnet date or writing deployable framework constructors.
        d['genesis_timestamp_seconds'] = 1_792_000_000
        d['acceptance_start_timestamp_seconds'] = 1_792_000_001
        p, ids, policy = op.render(json.loads(acceptance.PROFILE.read_text()), d, local=True)
        self.assertEqual(p['genesis']['purpose'], 'local-fixture-not-for-deployment')
        self.assertEqual(d['bootstrap_disburser']['transition'], 'disposable-helper-replacement')
        actors = [p['operator'], d['test_allocations'][0]['owner'], d['test_allocations'][2]['owner']]
        self.assertEqual(len(set(actors)), 3)
        self.assertNotIn(acceptance.PROTECTED_PERSONAL_PRINCIPAL, actors)
        self.assertEqual(d['voting_authority']['principal'], p['operator'])
        self.assertEqual(sum(n['stake'] for n in p['genesis']['neurons']), 100_000_000 * 10**8)
        self.assertEqual([n['memo'] for n in p['genesis']['neurons'][3:]], [10, 11, 20, 21])
        self.assertEqual(p['genesis']['neurons'][0]['permissions'][0]['permissions'], [2, 3, 4])
        self.assertEqual(set(ids.values()), set(policy['excluded_jup_neurons']))

    @unittest.skipUnless(os.environ.get('JUPITER_SNS_ARTIFACT_DIR'), 'exact official tuple required')
    def test_complete_candidate_packages_all_typed_inputs_and_verified_framework(self):
        # Synthetic owned identities stay inside a disposable unit-test directory;
        # this exercises the approved-input path, not a live acceptance package.
        d = local_decisions()
        d.update(approved=True, bootstrap_disburser=dict(transition='disposable-helper-replacement',
            state_and_pending_effects='Owned local unit fixture: no installation or effects',
            source_record='Synthetic unit-test data only'))
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / 'candidate'
            op.candidate(fixture(), d, os.environ['JUPITER_SNS_ARTIFACT_DIR'], out)
            self.assertEqual(len(list((out / 'applications').glob('*.bin'))), 7)
            self.assertEqual(len(list((out / 'sns').glob('*.bin'))), 5)
            self.assertEqual(len(list((out / 'framework-artifacts').glob('*.wasm.gz'))), 6)
            manifest = json.loads((out / 'manifest.json').read_text())
            self.assertFalse(manifest['canonical_production_release'])
            self.assertFalse(manifest['mainnet_operations_authorized'])
            self.assertFalse(manifest['live_observations_included'])
            for group in ('applications', 'sns'):
                self.assertIn('profile.json', json.loads((out / group / 'sha256.json').read_text()))
            policies = [json.loads((out / 'governance' / (stage + '-policy.json')).read_text())
                for stage in ('recognition', 'patron', 'treasury', 'treasury-continuation')]
            self.assertEqual([p['version'] for p in policies], [1, 2, 3, 4])
            for policy in policies[1:]:
                for field in ('protocol_governance_neuron', 'reserve_transport_neuron', 'excluded_jup_neurons'):
                    self.assertEqual(policy[field], policies[0][field])
            with self.assertRaisesRegex(ValueError, 'overwrite'):
                op.candidate(fixture(), d, os.environ['JUPITER_SNS_ARTIFACT_DIR'], out)

    @unittest.skipUnless(os.environ.get('JUPITER_SNS_ARTIFACT_DIR'), 'exact official tuple required')
    def test_complete_exact_genesis_and_native_recipes_round_trip(self):
        import sns
        p, ids, policy = op.render(fixture(), local_decisions(), local=True)
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)
            prepare.framework(p, os.environ['JUPITER_SNS_ARTIFACT_DIR'], out / 'sns', 1_792_000_000, local_fixture=True)
            prepare.applications(p, out / 'applications')
            p['_acceptance_start_timestamp_seconds'] = 1_792_000_000
            op.recipes(p, ids, policy, out / 'sns/governance.interface.did', out / 'governance')
            self.assertIn('"programme" = null', (out / 'applications/jupiter_sns_rewards.did').read_text())
            did = out / 'sns/governance.interface.did'
            # An actual supplied Wasm is required for an upgrade proposal. The
            # small module here checks only the official proposal wire contract.
            action = op.upgrade_action(p['roles']['jupiter_sns_rewards'], b'\0asm\x01\0\0\0',
                (out / 'governance/recognition-upgrade-args.bin').read_bytes())
            cd.encode_checked(did, '(ManageNeuron)', '(' + op.manage_value(ids['governance'], op.proposal(action, 'Local wire check')) + ')')
            with self.assertRaisesRegex(ValueError, 'chunked'):
                op.upgrade_action(p['roles']['jupiter_sns_rewards'], bytes(1_800_001), b'')


if __name__ == '__main__':
    unittest.main()
