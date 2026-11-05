import json
import os
from pathlib import Path
import tempfile
import unittest

import profile as acceptance
from test_profile import fixture
import sns
import candid as cd


def local_genesis(operator, other=None):
    genesis = json.loads((acceptance.REPO / 'tests/pocketic/fixtures/jup-sns-genesis.json').read_text())
    permissions = [{'principal': operator, 'permissions': list(range(1, 11))}]
    if other:
        permissions.append({'principal': other, 'permissions': [4]})
    genesis['neurons'] = [{'controller': operator, 'memo': 1, 'stake': 1_000_000_000,
                          'dissolve_delay': 31_536_000, 'permissions': permissions,
                          'voting_power_percentage_multiplier': 100,
                          'auto_stake_maturity': False, 'vesting_period_seconds': 0}]
    if other:
        genesis['neurons'].append({'controller': other, 'memo': 2, 'stake': 2_000_000_000,
            'dissolve_delay': 31_536_000,
            'permissions': [{'principal': other, 'permissions': [4]}],
            'voting_power_percentage_multiplier': 100,
            'auto_stake_maturity': False, 'vesting_period_seconds': 0})
    genesis['balances'] = [{'owner': operator, 'subaccountHex': None, 'amount': 100_000_000}]
    genesis['total_supply'] = sum(n['stake'] for n in genesis['neurons']) + 100_000_000
    return genesis


@unittest.skipUnless(os.environ.get('JUPITER_SNS_ARTIFACT_DIR'), 'exact official SNS artifacts required')
class OfficialConstructors(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.lock = json.loads((acceptance.REPO / 'deploy/acceptance/sns-framework.json').read_text())
        cls.directory = Path(os.environ['JUPITER_SNS_ARTIFACT_DIR'])
        cls.interfaces = sns.verify_artifacts(cls.lock, cls.directory)

    def test_tampered_artifact_rejected_before_construction(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)
            pin = self.lock['components']['governance']
            (out / pin['file']).write_bytes(b'corrupt')
            (out / 'governance.wasm.gz').write_bytes((self.directory / 'governance.wasm.gz').read_bytes())
            with self.assertRaisesRegex(ValueError, 'hash mismatch'):
                sns.verify_artifacts(self.lock, out)

    def test_both_project_inputs_use_the_same_official_constructor_contract(self):
        profile = fixture()
        identities = {role: profile['roles']['sns_' + role] for role in ('root', 'governance', 'ledger', 'index', 'swap')}
        identities.update(operator=profile['operator'], icp_ledger=acceptance.SYSTEM['ledger'],
                          nns_governance=acceptance.SYSTEM['governance'],
                          dapps=[profile['roles']['jupiter_historian']])
        for symbol, memo, stake, permissions in [('jUP', 1, 1_000_000_000, list(range(1, 11))),
                # IO's reviewed IOJUP001 vote-only neuron vector, not a default
                # shared allocation or a Jupiter economic choice.
                ('IO', 0x494f4a5550303031, 10_000_000_000_000_000, [4])]:
            with self.subTest(symbol=symbol), tempfile.TemporaryDirectory() as directory:
                genesis = local_genesis(profile['operator'])
                genesis['token_name'] = genesis['token_symbol'] = symbol
                genesis['neurons'][0].update(memo=memo, stake=stake,
                    permissions=[{'principal': profile['operator'], 'permissions': permissions}])
                genesis['total_supply'] = stake + 100_000_000
                out = Path(directory) / 'payloads'
                sns.constructors(self.lock, self.interfaces, identities, genesis, 1_792_000_000, out)
                for role in ('root', 'governance', 'ledger', 'index', 'swap'):
                    self.assertTrue((out / (role + '.bin')).read_bytes().startswith(b'DIDL'))
                # The pinned protobuf defines Normal=1, PreInitializationSwap=2.
                # Check the typed constructor, not a comment or fixture label.
                self.assertEqual(
                    cd.encode_checked(out / 'governance.interface.did', '(record { mode : int32 })',
                                      (out / 'governance.did').read_text()),
                    cd.encode_checked(out / 'governance.interface.did', '(record { mode : int32 })',
                                      '(record { mode = 1 : int32 })'))
                self.assertIn('"should_auto_finalize" = opt false', (out / 'swap.did').read_text())
                self.assertIn('"sns_token_e8s" = opt 0', (out / 'swap.did').read_text())
                self.assertIn('18446744073709551615', (out / 'swap.did').read_text())

    def test_fee_and_stake_cannot_disagree_with_governance(self):
        profile = fixture()
        identities = {role: profile['roles']['sns_' + role] for role in ('root', 'governance', 'ledger', 'index', 'swap')}
        identities['operator'] = profile['operator']
        identities['dapps'] = []
        genesis = local_genesis(profile['operator'])
        genesis['fee'] += 1
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, 'mismatch'):
                sns.constructors(self.lock, self.interfaces, identities, genesis, 1, Path(directory) / 'bad')


if __name__ == '__main__':
    unittest.main()
