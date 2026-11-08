import copy
import json
from pathlib import Path
import unittest

import profile as acceptance
from ic import account, principal_text, staking_subaccount


def fixture():
    profile = json.loads(acceptance.PROFILE.read_text())
    profile['operator'] = principal_text(bytes([51]) * 28 + b'\x02')
    profile['roles'] = {role: principal_text(bytes([n, 0, 1]))
                        for n, role in enumerate(acceptance.APPLICATIONS + acceptance.SNS, 10)}
    profile['nns_neuron']['id'] = 42
    profile['age_bonus_recipient_2'] = {'owner': profile['operator'], 'subaccountHex': None}
    profile['relay_surplus_neuron_recipients'] = []
    return profile


class Isolation(unittest.TestCase):
    def test_actual_handoff_keeps_uint64_precision_and_explicit_role_assignment_is_isolated(self):
        live = json.loads(acceptance.PROFILE.read_text())
        inventory = json.loads((acceptance.REPO / 'deploy/acceptance/provisioning.json').read_text())
        self.assertEqual(live['roles']['jupiter_disburser'], 'spoge-kaaaa-aaaas-amyoa-cai')
        self.assertEqual(live['nns_neuron']['id'], 2234355977000034918)
        self.assertEqual(str(live['nns_neuron']['id']), inventory['disburser']['nns_neuron_id'])
        self.assertNotIn(None, live['roles'].values())
        self.assertEqual(acceptance.validate_roles(live['roles']), live['roles'])
        self.assertEqual(set(live['roles'].values()) - {live['roles']['jupiter_disburser']},
                         set(inventory['initially_unassigned_canister_ids']))
        self.assertEqual(len(set(inventory['initially_unassigned_canister_ids'])), 11)
        for principal in [live['roles']['jupiter_disburser'], *inventory['initially_unassigned_canister_ids']]:
            acceptance.isolated_principal(principal)
        # Synthetic local role fixtures remain separate from the assigned live IDs.
        profile = fixture()
        profile['nns_neuron']['id'] = live['nns_neuron']['id']
        self.assertEqual(acceptance.validate(profile)['neuronId'], '2234355977000034918')
        import prepare
        import candid as cd
        _, arguments = prepare.application_arguments(profile)
        did = acceptance.REPO / 'canisters/disburser/jupiter_disburser.did'
        shape = '(record { neuron_id : nat64 })'
        self.assertEqual(cd.encode_checked(did, shape, '(' + arguments['jupiter_disburser'] + ')'),
                         cd.encode_checked(did, shape, '(record { neuron_id = 2234355977000034918 : nat64 })'))

    def test_unresolved_mainnet_profile_fails_closed(self):
        with self.assertRaisesRegex(ValueError, 'unresolved'):
            acceptance.validate(json.loads(acceptance.PROFILE.read_text()))

    def test_every_protected_application_is_rejected_in_every_role(self):
        for role in acceptance.APPLICATIONS + acceptance.SNS:
            for protected in acceptance.PROTECTED:
                with self.subTest(role=role, protected=protected):
                    profile = fixture()
                    profile['roles'][role] = protected
                    with self.assertRaisesRegex(ValueError, 'protected'):
                        acceptance.validate(profile)

    def test_protected_neurons_and_financial_accounts(self):
        for neuron in acceptance.PROTECTED_NEURONS:
            profile = fixture()
            profile['nns_neuron']['id'] = neuron
            with self.assertRaisesRegex(ValueError, 'production'):
                acceptance.validate(profile)
        for sub in acceptance.PROTECTED_STAKING:
            profile = fixture()
            profile['age_bonus_recipient_2'] = {
                'owner': acceptance.SYSTEM['governance'], 'subaccountHex': sub.upper()}
            with self.assertRaisesRegex(ValueError, 'production'):
                acceptance.validate(profile)
        profile = fixture()
        profile['age_bonus_recipient_2'] = {'owner': acceptance.PROTECTED[0], 'subaccountHex': None}
        with self.assertRaisesRegex(ValueError, 'allowlist'):
            acceptance.validate(profile)
        for sub in ('00' * 32, ' '.join(acceptance.PROTECTED_STAKING[0][i:i+2] for i in range(0, 64, 2))):
            profile = fixture()
            profile['age_bonus_recipient_2'] = {'owner': acceptance.SYSTEM['governance'], 'subaccountHex': sub}
            with self.assertRaises(ValueError):
                acceptance.validate(profile)

    def test_shared_dquorum_is_an_explicit_exact_third_party_recipient(self):
        # Exact reviewed production Disburser beneficiary; not IO's permanent neuron.
        sub = '77e63de72b5e3339ea20f4baf3ec2bd92138ddde0daeb69db50acceb384bdf0f'
        profile = fixture()
        profile['age_bonus_recipient_2'] = {
            'owner': acceptance.SYSTEM['governance'], 'subaccountHex': sub.upper()}
        self.assertEqual(acceptance.validate(profile)['ageBonusRecipient2'],
                         account(acceptance.SYSTEM['governance'], sub))
        import prepare
        import candid as cd
        _, args = prepare.application_arguments(profile)
        did = acceptance.REPO / 'canisters/disburser/jupiter_disburser.did'
        shape = '(record { age_bonus_recipient_2 : Account })'
        expected = cd.record(age_bonus_recipient_2=cd.account(profile['age_bonus_recipient_2']))
        self.assertEqual(cd.encode_checked(did, shape, '(' + args['jupiter_disburser'] + ')'),
                         cd.encode_checked(did, shape, '(' + expected + ')'))
        # Preparing this choice is not permission to transfer real ICP.
        checked_in = json.loads(acceptance.PROFILE.read_text())
        self.assertEqual(checked_in['age_bonus_recipient_2'], {
            'owner': acceptance.SYSTEM['governance'],
            'subaccountHex': acceptance.DQUORUM_STAKING_SUBACCOUNT})
        self.assertIsNone(checked_in['relay_surplus_neuron_recipients'])

    def test_other_governance_accounts_and_wrong_dquorum_owner_remain_rejected(self):
        for sub in (None, '00' * 32, '01' * 32,
                    staking_subaccount('oae4c-3iaaa-aaaar-qb5qq-cai', 0)):
            with self.subTest(subaccount=sub):
                profile = fixture()
                profile['age_bonus_recipient_2'] = {
                    'owner': acceptance.SYSTEM['governance'], 'subaccountHex': sub}
                with self.assertRaises(ValueError):
                    acceptance.validate(profile)
        profile = fixture()
        profile['age_bonus_recipient_2'] = {
            'owner': acceptance.SYSTEM['ledger'],
            'subaccountHex': '77e63de72b5e3339ea20f4baf3ec2bd92138ddde0daeb69db50acceb384bdf0f'}
        with self.assertRaisesRegex(ValueError, 'allowlist'):
            acceptance.validate(profile)

    def test_duplicate_system_invalid_and_operator_ids(self):
        for value in (acceptance.SYSTEM['ledger'], 'aaaaa-aa', '2vxsx-fae',
                      'invalid', fixture()['operator'], fixture()['roles']['sns_root']):
            profile = fixture()
            profile['roles']['jupiter_faucet'] = value
            with self.assertRaises(ValueError):
                acceptance.validate(profile)

    def test_no_unknown_profile_fields_or_silent_network_changes(self):
        for key, value in [('network', 'local'), ('environment', 'ic'), ('unknown', True)]:
            profile = fixture()
            profile[key] = value
            with self.assertRaises(ValueError):
                acceptance.validate(profile)

    def test_canonical_production_account_vector(self):
        sub = staking_subaccount('uccpi-cqaaa-aaaar-qby3q-cai', 0)
        self.assertEqual(sub, acceptance.PROTECTED_STAKING[0])
        value = account(acceptance.SYSTEM['governance'], sub)
        self.assertEqual(value['explorerAccountHex'], '22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d')
        self.assertEqual(value['address'], 'rrkah-fqaaa-aaaaa-aaaaq-cai-h7evq5y.' + sub)

    def test_io_and_jupiter_neuron_derivation_vectors(self):
        # Fixed IO vectors from its accepted constructor test and source-bound
        # sale-free genesis account record (8b2e1cba).
        self.assertEqual(staking_subaccount('gosbn-sqaaa-aaaas-amxua-cai', 0),
                         '07734f006ddcefc43f9ec1ebcdf0e053811ffd03bbf0b06df8df4a874e3e51d0')
        self.assertEqual(staking_subaccount('zkkkd-i34qc-367ln-e2u7o-ezznu-dkfqh-gtfvz-cviph-6qa4v-evtfs-wqe', 0x494f4a5550303031),
                         '8d56e3b80021a892f55e161e8725092d866a5f9580badb2d408ebe09dbae5eef')
        # IO's accepted permanent-neuron nonce is also zero; no IO economics enter
        # the shared account derivation. Check both actual controller inputs.
        for controller in ('gosbn-sqaaa-aaaas-amxua-cai', fixture()['roles']['jupiter_disburser']):
            sub = staking_subaccount(controller, 0)
            self.assertEqual(len(bytes.fromhex(sub)), 32)
            self.assertNotIn(sub, acceptance.PROTECTED_STAKING)
            self.assertNotEqual(account(acceptance.SYSTEM['governance'], sub)['explorerAccountHex'],
                                '22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d')

    def test_frontend_and_compiled_bindings_are_the_same_profile(self):
        profile = fixture()
        before = copy.deepcopy(profile)
        front, build = acceptance.frontend(profile), acceptance.bindings(profile)
        self.assertEqual(front['relayCanisterId'], build['JUPITER_BOUND_RELAY_ID'])
        self.assertEqual(front['neuronId'], build['JUPITER_BOUND_FAUCET_NEURON_ID'])
        self.assertEqual(front['historianCanisterId'], profile['roles']['jupiter_historian'])
        self.assertIsNone(front['ioStakingSubaccountHex'])
        self.assertEqual(profile, before)

    def test_sns_genesis_does_not_replace_the_nns_neuron_binding(self):
        from test_sns import local_genesis
        profile = fixture()
        profile['genesis'] = local_genesis(profile['operator'])
        self.assertEqual(acceptance.validate(profile)['neuronId'], '42')

    def test_acceptance_markup_has_no_production_application_or_staking_links(self):
        source = (acceptance.REPO / 'canisters/frontend/public/index.html').read_text()
        rendered = acceptance.render_markup(fixture(), source)
        for value in (acceptance.PROTECTED[:7] + tuple(map(str, acceptance.PROTECTED_NEURONS))
                      + acceptance.PROTECTED_STAKING + (acceptance.DQUORUM_STAKING_SUBACCOUNT,)):
            self.assertNotIn(value, rendered)
        self.assertIn(acceptance.frontend(fixture())['stakingAccount']['explorerAccountHex'], rendered)

    def test_generated_application_constructors_use_the_isolated_profile(self):
        import prepare
        import tempfile
        import subprocess
        profile = fixture()
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / 'constructors'
            prepare.applications(profile, out)
            for role in acceptance.APPLICATIONS:
                self.assertTrue((out / (role + '.bin')).is_file())
                decoded = subprocess.check_output(['didc', 'decode', (out / (role + '.bin')).read_bytes().hex()], text=True)
                for value in acceptance.PROTECTED[:7] + tuple(map(str, acceptance.PROTECTED_NEURONS)):
                    self.assertNotIn(value, decoded)
            self.assertIn(profile['roles']['sns_root'], (out / 'jupiter_sns_rewards.did').read_text())
            self.assertIn(profile['roles']['jupiter_relay'], (out / 'jupiter_historian.did').read_text())

    def test_missing_application_recipient_policy_cannot_generate_payloads(self):
        import prepare
        import tempfile
        for field in ('age_bonus_recipient_2', 'relay_surplus_neuron_recipients'):
            profile = fixture()
            profile[field] = None
            with self.subTest(field=field), tempfile.TemporaryDirectory() as directory:
                out = Path(directory) / 'constructors'
                with self.assertRaisesRegex(ValueError, 'explicit decisions'):
                    prepare.applications(profile, out)
                self.assertFalse(out.exists())

    def test_missing_genesis_and_local_economics_cannot_prepare_live_sns(self):
        import prepare
        profile = fixture()
        with self.assertRaisesRegex(ValueError, 'unresolved'):
            prepare.framework(profile, '/unused', '/unused', 0)
        profile['genesis'] = {'purpose': 'local-fixture-not-for-deployment', 'balances': [], 'neurons': []}
        with self.assertRaisesRegex(ValueError, 'local economics'):
            prepare.framework(profile, '/unused', '/unused', 0)


if __name__ == '__main__':
    unittest.main()
