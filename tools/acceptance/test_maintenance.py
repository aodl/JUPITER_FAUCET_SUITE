import copy
import fcntl
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import subprocess
import candid as cd

import maintenance
import observe
import profile as acceptance


def profile():
    return json.loads(acceptance.PROFILE.read_text())


def evidence():
    p = profile()
    rows = [dict(target=target, method=method, args=args, ok=True,
                 value='fixture', verification='local fixture only')
            for target, method, args in observe.slots(p)]
    for row in rows:
        if row['method'] == 'get_nervous_system_parameters':
            row['value'] = dict(automatically_advance_target_version=[False])
        elif row['method'] == 'get_running_sns_version':
            row['value'] = dict(deployed_version=[], pending_version=[])
        elif row['method'] == 'get_upgrade_journal':
            row['value'] = dict(target_version=[], deployed_version=[], upgrade_journal_entry_count=['0'])
        elif row['method'] == 'get_programme_status':
            row['value'] = dict(config=[], last_error=[], verified_e8s='0', assumed_e8s='0', failed_items='0')
    return dict(kind='local-fixture-not-live', network='ic', caller='2vxsx-fae',
                observations=rows, traffic=[], complete=True)


class Maintenance(unittest.TestCase):
    def test_explicit_isolated_topology_and_deterministic_plan(self):
        p = profile()
        plan = maintenance.plan(p)
        self.assertEqual(plan, json.loads((acceptance.PROFILE.parent/'maintenance-plan.json').read_text()))
        self.assertEqual(len(plan['framework']), 5)
        for row in plan['framework']:
            self.assertEqual(row['after_rehearsal']['controllers'][-1], p['operator'])
            self.assertNotIn(acceptance.PROTECTED_PERSONAL_PRINCIPAL, row['after_rehearsal']['controllers'])
            self.assertIn(acceptance.PROTECTED_PERSONAL_PRINCIPAL, row['add_before_remove']['controllers'])
        self.assertIsNone(plan['viewer']['principal'])
        for key, value in [('environment','production'),('operator',acceptance.PROTECTED_PERSONAL_PRINCIPAL)]:
            bad = copy.deepcopy(p); bad[key] = value
            with self.assertRaises(ValueError): maintenance.plan(bad)
        for change in [dict(approved=True),dict(viewer=p['operator']),dict(executor=acceptance.PROTECTED_PERSONAL_PRINCIPAL)]:
            bad=copy.deepcopy(p); bad['maintenance'].update(change)
            with self.assertRaises(ValueError): acceptance.validate(bad)
        from ic import principal_text
        bad=copy.deepcopy(p);bad['roles']['sns_root']=principal_text(b'\x31\x00\x01')
        with self.assertRaisesRegex(ValueError,'fixed twelve acceptance assignments'):
            maintenance.plan(bad)

    def test_no_clock_or_economic_changes(self):
        p = profile()
        candidate=json.loads((acceptance.PROFILE.parent/'candidate-inputs.json').read_text())
        self.assertIsNone(candidate['genesis_timestamp_seconds'])
        self.assertIsNone(candidate['acceptance_start_timestamp_seconds'])
        self.assertEqual(sum(n['amount_e8s'] for n in candidate['test_allocations']),20_000*10**8)
        records=json.loads((acceptance.PROFILE.parent/'proposed-operations-2026-11-15.json').read_text())
        for row in records['controller_transitions']:
            if row['role'] in acceptance.SNS:
                self.assertEqual(len(row['proposed_stages'][-1]['controllers']),2)
        self.assertEqual(p['relay_surplus_neuron_recipients'],[])

    def test_native_proposal_selector_is_bounded_and_never_executes(self):
        rows=[dict(id=i,status='OPEN',proposal=dict(action=dict(ExecuteNnsFunction=dict(nns_function=code,payload_hex='4449444c0000')))) for i,code in enumerate([30,37,1])]
        selected=maintenance.select(dict(proposal_info=rows))
        self.assertEqual([r['kind'] for r in selected],['AddSnsWasm','InsertSnsWasmUpgradePathEntries'])
        self.assertTrue(all('never install or vote' in r['disposition'] for r in selected))
        with self.assertRaises(ValueError):maintenance.select(dict(proposal_info=rows*17))

    def test_arbitrary_candidate_manifest_cannot_claim_reviewed_official_provenance(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'unreviewed.json';path.write_text('{}')
            out=Path(directory)/'candidate'
            with self.assertRaisesRegex(ValueError,'unreviewed candidate lock'):
                maintenance.candidate(path,Path(directory)/'untrusted-wasm',out)
            self.assertFalse(out.exists())

    def test_typed_controller_plan_and_opt_in_exercises(self):
        with tempfile.TemporaryDirectory() as directory:
            result=maintenance.write_plan(profile(),Path(directory)/'typed')
            for row in result['framework']:
                for stage in ('add_before_remove','after_rehearsal'):
                    path=Path(directory)/'typed'/row[stage]['argument_hex_file']
                    self.assertTrue(bytes.fromhex(path.read_text()).startswith(b'DIDL'))
        with self.assertRaises(ValueError):maintenance.exercises('')
        result=maintenance.exercises('local-fixture-001')
        self.assertTrue(result['dry_run'])
        self.assertFalse(result['approved'])
        self.assertEqual(result['max_transfer_submission_attempts_per_exercise'],1)
        self.assertEqual(result['retries'],0)
        self.assertEqual([r['proposed_max_debit_e8s'] for r in result['exercises']],
                         [100010000,100010000,100010000,0,100010000])
        self.assertTrue(all(r['operation']['maximum_authorized_spend']==0 for r in result['exercises']))
        self.assertNotEqual(result['exercises'][2]['operation']['memo_hex'],
                            result['exercises'][0]['operation']['memo_hex'])

    def test_sdk_projections_decode_pinned_official_and_application_types_offline(self):
        p=profile(); roles=p['roles']; root=acceptance.PROFILE.parents[2]
        gov=root/'candid/sns-governance/governance.did'
        app=root/'canisters/sns-rewards/jupiter_sns_rewards.did'
        lock=json.loads((acceptance.PROFILE.parent/'sns-framework.json').read_text())
        version=cd.record(**{r+'_wasm_hash':cd.blob(bytes.fromhex(pin['raw_sha256']))
                             for r,pin in lock['components'].items()})
        config=cd.record(version='1', patron_enabled='false', launch_timestamp_seconds='opt 123',
            staking_account=cd.account(dict(owner=acceptance.SYSTEM['governance'],subaccountHex='01'*32)),
            protocol_governance_neuron=cd.blob(b'\x01'),excluded_jup_neurons='vec {}',
            excluded_icp_accounts='vec {}',jup_root=cd.principal(roles['sns_root']),
            canonical_relay=cd.principal(roles['jupiter_relay']),icp_ledger=cd.principal(acceptance.SYSTEM['ledger']),
            icp_index=cd.principal(acceptance.SYSTEM['index']),snapshot_max_age_seconds='3600')
        runtime=cd.record(config='opt '+config,last_error='null',verified_e8s='0',assumed_e8s='0',failed_items='0',
                          next_snapshot='1')
        cases=[('get_nervous_system_parameters',gov,'NervousSystemParameters','record { automatically_advance_target_version = opt false }'),
               ('get_running_sns_version',gov,'GetRunningSnsVersionResponse',cd.record(deployed_version='opt '+version)),
               ('get_upgrade_journal',gov,'GetUpgradeJournalResponse',cd.record(target_version='opt '+version)),
               ('get_programme_status',app,'Runtime',runtime)]
        payloads={name:cd.encode_checked(did,'('+types+')','('+value+')').hex() for name,did,types,value in cases}
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'replies.json';path.write_text(json.dumps(payloads))
            js="""import fs from 'node:fs';
import {IDL} from '@icp-sdk/core/candid';
import {publicQueries} from './tools/acceptance/observe.mjs';
const replies=JSON.parse(fs.readFileSync(process.argv[1]));
const decoded=Object.fromEntries(publicQueries({}).map(([,method,,,types])=>[method,IDL.decode(types,new Uint8Array(Buffer.from(replies[method],'hex')))[0]]));
console.log(JSON.stringify(decoded,(_,v)=>typeof v==='bigint'?v.toString():v));"""
            result=subprocess.run(['node','--input-type=module','-e',js,str(path)],cwd=root,capture_output=True,text=True,timeout=45)
            self.assertEqual(result.returncode,0,result.stderr)
        values=json.loads(result.stdout)
        self.assertEqual(values['get_nervous_system_parameters']['automatically_advance_target_version'],[False])
        self.assertEqual(values['get_programme_status']['config'][0]['version'],'1')
        self.assertEqual(values['get_programme_status']['config'][0]['launch_timestamp_seconds'],['123'])

    def test_failed_partial_epoch_and_restart(self):
        p, e = profile(), evidence()
        good=observe.report(p,e,None,10000,900)
        self.assertTrue(good['checkpoint_eligible'])
        self.assertEqual(good['coverage']['direct_patron'],'UNAVAILABLE_EVIDENCE')
        self.assertEqual(good['coverage']['io_settlement'],'NOT_ENABLED')
        e['observations'][0]['ok']=False
        e['observations'][0].pop('value')
        partial=observe.report(p,e,good,11000,900)
        self.assertFalse(partial['checkpoint_eligible'])
        self.assertNotIn('BLOCKED',[a['category'] for a in partial['alerts']])
        e=evidence();e['observations'][0]['value']='new module'
        changed=observe.report(p,e,good,12000,900)
        self.assertFalse(changed['checkpoint_eligible'])
        self.assertEqual({a['category'] for a in changed['alerts']},{'DELAYED','BLOCKED'})
        fee_change=evidence()
        next(r for r in fee_change['observations'] if r['method']=='icrc1_fee')['value']='10001'
        changed=observe.report(p,fee_change,good,11000,900)
        self.assertFalse(changed['checkpoint_eligible'])
        self.assertIn('BLOCKED',[a['category'] for a in changed['alerts']])
        with self.assertRaises(ValueError):observe.report(p,e,good,9999,900)
        e['observations'][0]['target']=acceptance.PROTECTED[0]
        with self.assertRaises(ValueError):observe.report(p,e,None,10000,900)
        e=evidence();e['observations'][0]['method']='icrc1_transfer'
        with self.assertRaises(ValueError):observe.report(p,e,None,10000,900)
        e=evidence();e['observations'][-1]=e['observations'][-2]
        with self.assertRaises(ValueError):observe.report(p,e,None,10000,900)

    def test_checkpoint_not_advanced_on_failure_and_overlap_denied(self):
        with tempfile.TemporaryDirectory() as directory:
            out=Path(directory);fixture=out/'input.json'
            fixture.write_text(json.dumps(evidence()))
            self.assertEqual(observe.run(acceptance.PROFILE,out,fixture),0)
            before=(out/'fixture-checkpoint.json').read_bytes()
            e=evidence();e['complete']=False;fixture.write_text(json.dumps(e))
            self.assertEqual(observe.run(acceptance.PROFILE,out,fixture),2)
            self.assertEqual((out/'fixture-checkpoint.json').read_bytes(),before)
            with (out/'.lock').open('a') as lock:
                fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
                with self.assertRaises(BlockingIOError):observe.run(acceptance.PROFILE,out,fixture)

    def test_native_version_policy_and_collection_timeout_are_not_green(self):
        for method, value in [
                ('get_nervous_system_parameters',dict(automatically_advance_target_version=[])),
                ('get_running_sns_version',dict(pending_version=[dict(proposal_id='42')])),
                ('get_programme_status',dict(last_error=['fixture error']))]:
            e=evidence()
            next(r for r in e['observations'] if r['method']==method)['value']=value
            result=observe.report(profile(),e,None,10000,900)
            self.assertFalse(result['checkpoint_eligible'])
            self.assertIn('BLOCKED',[a['category'] for a in result['alerts']])
        with tempfile.TemporaryDirectory() as directory:
            with patch('observe.subprocess.run',side_effect=subprocess.TimeoutExpired('node',210)):
                self.assertEqual(observe.run(acceptance.PROFILE,directory,collect=True),2)
            self.assertFalse((Path(directory)/'checkpoint.json').exists())
            result=json.loads((Path(directory)/'report.json').read_text())
            self.assertEqual(result['coverage']['direct_patron'],'UNAVAILABLE_EVIDENCE')

    def test_independent_exact_receipt_and_negative_arithmetic(self):
        expected=dict(source_block=42,destination='exact-account',amount=12345,fee=100,memo='exact-memo')
        receipt=dict(complete=True,canonical=True,transactions=[copy.deepcopy(expected)])
        self.assertEqual(observe.reconcile(expected,receipt),'HEALTHY')
        for key,value in [('destination','wrong'),('amount',12346),('fee',101),('memo','wrong'),('source_block',43)]:
            bad=copy.deepcopy(receipt);bad['transactions'][0][key]=value
            self.assertEqual(observe.reconcile(expected,bad),'INVARIANT_FAILURE')
        bad=copy.deepcopy(receipt);bad['transactions']*=2
        self.assertEqual(observe.reconcile(expected,bad),'INVARIANT_FAILURE')
        for key in ('complete','canonical'):
            bad=copy.deepcopy(receipt);bad[key]=False
            self.assertEqual(observe.reconcile(expected,bad),'UNAVAILABLE_EVIDENCE')
        self.assertEqual(observe.reconcile(dict(expected,dispatch_state='AssumedPaidUnverified'),receipt),'UNAVAILABLE_EVIDENCE')
        self.assertEqual(observe.reconcile(dict(expected,owner_snapshot_fresh=False),receipt),'UNAVAILABLE_EVIDENCE')
        self.assertEqual(observe.reconcile(dict(expected,compounded=True),receipt),'DELAYED')


if __name__ == '__main__':
    unittest.main()
