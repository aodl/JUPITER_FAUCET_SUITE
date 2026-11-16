import copy
import json
import os
from pathlib import Path
import tempfile
import time
import unittest
from unittest.mock import patch

import campaign
import maintenance
import observe
import operational
import profile as acceptance
import viewer
from ic import principal_text, staking_subaccount


def fixture(name='direct liquid'):
    p=json.loads(acceptance.PROFILE.read_text())
    inputs=json.loads((acceptance.PROFILE.parent/'candidate-inputs.json').read_text())
    accounts=json.loads((acceptance.PROFILE.parent/'derived-accounts.json').read_text())
    operation=next(r['operation'] for r in maintenance.exercises('fixture-001')['exercises'] if r['name']==name)
    participant=inputs['test_allocations'][2 if name=='direct 2x' else 0]['owner']
    stake=accounts['nns_staking_expectation']['account']
    memo=(b'jUP-patron-v1'+(40).to_bytes(8,'big')+(1).to_bytes(8,'big')).hex()
    to={'owner':p['roles']['sns_governance'],'subaccountHex':staking_subaccount(participant,21)} if name=='direct 2x' else {'owner':participant if name=='direct liquid' else p['roles']['jupiter_relay']}
    amount=20_000_000 if name=='direct 2x' else 10_000_000
    deposit=dict(block=40,kind='transfer',amount=100_000_000,fee=10000,from_={'owner':participant},to=stake,memo_hex=operation.get('memo_hex'),timestamp_ns=2_000_000_000_000)
    deposit['from']=deposit.pop('from_')
    receipt=dict(block=60,kind='transfer',amount=amount,fee=1_000_000,to=to,memo_hex=memo,
                 **{'from':{'owner':p['roles']['jupiter_sns_rewards']}})
    record=dict(contribution_block=40,qualifying_icp_e8s=100_000_000,commitment_timestamp_seconds=2000,
                commitment_memo_hex=operation.get('memo_hex'),launch_timestamp_seconds=1000,amount_e8s=amount,
                fee_e8s=1_000_000,to=to,memo_hex=memo,snapshot_id=1,outcome='Verified',
                config_version=2,selection_at_ns=3_000_000_000_000,snapshot_completed_at_seconds=2900)
    neuron=dict(id=staking_subaccount(participant,21 if name=='direct 2x' else 11),
                cached_neuron_stake_e8s=5000*10**8,neuron_fees_e8s=0,permissions=[dict(principal=participant,permissions=list(range(1,11)))],
                dissolve_delay_seconds=31557600,auto_stake_maturity=name=='direct 2x')
    def src(label,target,method,value,**kw):
        return dict(id=label,target=target,method=method,verification='local-fixture',value=value,**kw)
    sources=[src('icp',acceptance.SYSTEM['ledger'],'query_blocks',dict(log_length=41,transactions=[deposit])),
             src('jup',p['roles']['sns_ledger'],'get_transactions',dict(log_length=61,transactions=[receipt])),
             src('patron',p['roles']['jupiter_sns_rewards'],'get_patron_record',record),
             src('selection',p['roles']['sns_governance'],'get_neuron',neuron,captured_at_ns=record['selection_at_ns'])]
    sources.append(src('programme',p['roles']['jupiter_sns_rewards'],'get_programme_status',dict(config=dict(version=2,launch_timestamp_seconds=1000,patron_enabled=True))))
    exercise=dict(programme_source='programme',name=name,source_block=40,source_timestamp_ns=deposit['timestamp_ns'],source_account={'owner':participant},
                  approved_icp_fee_e8s=10000,approved_jup_fee_e8s=1_000_000,patron_source='patron',selection_source='selection')
    return dict(schema=1,campaign_id='fixture-001',sources=sources,exercises=[exercise],
                windows=[dict(ledger=acceptance.SYSTEM['ledger'],start=40,end_exclusive=41,sources=['icp']),
                         dict(ledger=p['roles']['sns_ledger'],start=60,end_exclusive=61,sources=['jup'])])


class Campaign(unittest.TestCase):
    def test_exact_imported_receipts_are_deterministic(self):
        f=fixture();report=campaign.verify(f)
        self.assertEqual(report,campaign.verify(copy.deepcopy(f)))
        self.assertEqual(report['status'],'PASS')
        # Native ICP blocks expose legacy AccountIdentifiers directly.
        tx=f['sources'][0]['value']['transactions'][0]
        tx['from']=campaign.addr(tx['from']);tx['to']=campaign.addr(tx['to'])
        self.assertEqual(campaign.verify(f)['status'],'PASS')
        # A's ordinary recognition may use either approved zero/year neuron;
        # zero delay does not itself qualify for voting/2x compounding.
        f['sources'][3]['value']['id']=staking_subaccount(f['exercises'][0]['source_account']['owner'],10)
        f['sources'][3]['value']['dissolve_delay_seconds']=0
        self.assertEqual(campaign.verify(f)['status'],'PASS')
        self.assertTrue(report['no_network'] and report['no_signing'] and report['no_replay'])
        self.assertEqual(report['exercises'][0]['checks']['jup_settlement'],'HEALTHY')

    def test_wrong_transaction_amount_fee_destination_memo_and_source_fail(self):
        for key,value in [('amount',10_000_001),('fee',1_000_001),('to',{'owner':acceptance.SYSTEM['ledger']}),
                          ('memo_hex','ff'),('from',{'owner':acceptance.SYSTEM['ledger']})]:
            f=fixture(); f['sources'][1]['value']['transactions'][0][key]=value
            self.assertEqual(campaign.verify(f)['status'],'FAIL',key)
        missing=fixture();missing['sources'][1]['value']['transactions'][0]['memo_hex']='ff'
        self.assertEqual(campaign.verify(missing)['exercises'][0]['checks']['reserve_source_and_fee'],'UNAVAILABLE_EVIDENCE')
        f=fixture();f['sources'][0]['value']['transactions'][0]['amount']=1
        self.assertEqual(campaign.verify(f)['status'],'FAIL')

    def test_duplicate_event_detected_but_missing_archive_and_uncertainty_incomplete(self):
        f=fixture();r=copy.deepcopy(f['sources'][1]['value']['transactions'][0]);r['block']=61
        f['sources'][1]['value']['transactions'].append(r);f['sources'][1]['value']['log_length']=62;f['windows'][1]['end_exclusive']=62
        self.assertEqual(campaign.verify(f)['status'],'FAIL')
        for outcome in ('AssumedPaidUnverified','Dispatched','Uncertain'):
            f=fixture();f['sources'][2]['value']['outcome']=outcome
            self.assertEqual(campaign.verify(f)['status'],'INCOMPLETE',outcome)
        f=fixture();f['sources'][1]['value']['transactions']=[]
        report=campaign.verify(f)
        self.assertEqual(report['status'],'INCOMPLETE')
        self.assertEqual(report['exercises'][0]['checks']['jup_settlement'],'UNAVAILABLE_EVIDENCE')

    def test_authentic_archive_callback_completion(self):
        f=fixture();page=f['sources'][1]['value'];row=page['transactions'].pop()
        archive=principal_text(b'\x11\x01')
        page['archived_ranges']=[dict(target=archive,start=60,end_exclusive=61)]
        f['sources'].append(dict(id='archive',target=archive,method='get_transactions',verification='local-fixture',value=dict(transactions=[row])))
        self.assertEqual(campaign.verify(f)['status'],'INCOMPLETE')
        f['windows'][1]['sources'].append('archive')
        self.assertEqual(campaign.verify(f)['status'],'PASS')
        f['sources'][-1]['value']['transactions'][0]['block']=99
        with self.assertRaises(ValueError): campaign.verify(f)

    def test_compound_settlement_separate_from_refresh_and_historical_selection(self):
        f=fixture('direct 2x');report=campaign.verify(f)
        self.assertEqual(report['status'],'INCOMPLETE')
        self.assertEqual(report['exercises'][0]['checks']['jup_settlement'],'HEALTHY')
        f['exercises'][0]['refresh_source']='selection';f['exercises'][0]['expected_cached_stake_e8s']=5000*10**8+20_000_000
        self.assertEqual(campaign.verify(f)['exercises'][0]['checks']['subsequent_cached_stake_refresh'],'DELAYED')
        f['sources'][3]['captured_at_ns']+=1
        self.assertEqual(campaign.verify(f)['exercises'][0]['checks']['frozen_owner_eligibility'],'UNAVAILABLE_EVIDENCE')
        f=fixture('direct 2x');f['sources'][3]['value']['auto_stake_maturity']=False
        self.assertEqual(campaign.verify(f)['status'],'FAIL')

    def test_relay_pot_never_implies_individual_entitlement(self):
        report=campaign.verify(fixture('Relay pot'))
        self.assertEqual(report['status'],'INCOMPLETE')
        checks=report['exercises'][0]['checks']
        self.assertEqual(checks['relay_aggregate_pot'],'HEALTHY')
        self.assertEqual(checks['individual_relay_entitlement'],'UNAVAILABLE_EVIDENCE')

    def test_native_proposal_and_treasury_evidence_match_known_operations(self):
        p=json.loads(acceptance.PROFILE.read_text())
        f=fixture();e=f['exercises'][0]
        e.update(name='harmless native proposal/vote',proposal_source='proposal',proposal_id=1)
        proposal=dict(id=1,executed_timestamp_seconds=2001,failed_timestamp_seconds=0,
            action=dict(kind='ManageSnsMetadata'),ballots={staking_subaccount(p['operator'],1):dict(vote=1)})
        f['sources'].append(dict(id='proposal',target=p['roles']['sns_governance'],method='get_proposal',
                                verification='local-fixture',value=proposal))
        report=campaign.verify(f)
        self.assertEqual(report['exercises'][0]['checks']['native_proposal_execution'],'HEALTHY')
        self.assertEqual(report['status'],'INCOMPLETE')  # ballot cannot prove signing principal
        proposal['action']['kind']='TransferSnsTreasuryFunds'
        self.assertEqual(campaign.verify(f)['status'],'FAIL')
        e['name']='bounded treasury stake'
        accounts=json.loads((acceptance.PROFILE.parent/'derived-accounts.json').read_text())
        gov=accounts['sns_governance']['account']
        e['source_account']=gov
        tx=f['sources'][0]['value']['transactions'][0]
        tx['from']=gov;tx['memo_hex']=None
        proposal['action'].update(from_treasury='ICP',to=tx['to'],amount_e8s=100_000_000,memo=None)
        report=campaign.verify(f)
        self.assertEqual(report['exercises'][0]['checks']['source_transaction'],'HEALTHY')
        self.assertEqual(report['exercises'][0]['checks']['executed_treasury_proposal'],'HEALTHY')
        self.assertEqual(report['exercises'][0]['checks']['proposal_to_transfer_attribution'],'UNAVAILABLE_EVIDENCE')
        proposal['id']=2
        self.assertEqual(campaign.verify(f)['status'],'FAIL')
        proposal['id']=1;tx['memo_hex']='not-a-null-memo'
        self.assertEqual(campaign.verify(f)['status'],'FAIL')

    def test_decimal_oracle_whole_halves_and_fractional_day(self):
        self.assertEqual(campaign.reward_range(1_000_000_000,1461*86400,0,True),(100_000_000,100_000_000))
        low,high=campaign.reward_range(1_000_000_000,86400,0,False)
        self.assertEqual(high-low,1)
        self.assertLess(high,100_000_000)

    def test_current_lookups_do_not_prove_historic_owner_or_stale_snapshot(self):
        live=fixture();live['sources'][2]['verification']='replica-signed-query';live['sources'][3]['verification']='replica-signed-query'
        self.assertEqual(campaign.verify(live)['exercises'][0]['checks']['frozen_owner_eligibility'],'UNAVAILABLE_EVIDENCE')
        del live['sources'][2]['value']['selection_at_ns']
        self.assertEqual(campaign.verify(live)['status'],'INCOMPLETE')
        f=fixture();f['sources'][3]['captured_at_ns']=4000*10**9
        self.assertEqual(campaign.verify(f)['status'],'INCOMPLETE')
        f=fixture();f['sources'][3]['value']['permissions'][0]['permissions'].append(11)
        self.assertEqual(campaign.verify(f)['status'],'FAIL')
        f=fixture();f['sources'][2]['value']['snapshot_completed_at_seconds']=-999999
        self.assertEqual(campaign.verify(f)['status'],'FAIL')


class Viewer(unittest.TestCase):
    def approval(self):
        p=json.loads(acceptance.PROFILE.read_text())
        return dict(principal=principal_text(bytes([41])*29),dfx_identity_alias='test_viewer',approved=True,
                    restricted_host_confirmed=True,network='ic',dfx_version='0.31.0',targets=list(p['roles'].values()))

    def test_exact_provider_registry_and_no_mutating_methods(self):
        approval=self.approval();commands=viewer.commands(approval)
        self.assertEqual(len(commands),24)
        self.assertEqual({x['method'] for x in commands},{'canister_status','fetch_canister_logs'})
        self.assertTrue(all('--candid' in x['command'] and '--identity' in x['command'] for x in commands))
        for field,value in [('principal',acceptance.PROTECTED_PERSONAL_PRINCIPAL),('approved',False),('dfx_identity_alias','codex_local')]:
            bad=dict(approval,**{field:value})
            with self.assertRaises(ValueError):viewer.commands(bad)
        # No provider is invoked or identity generated by these tests.
        with tempfile.TemporaryDirectory() as directory:
            viewer.settings(approval['principal'],Path(directory)/'settings')
            self.assertEqual(len(list((Path(directory)/'settings').glob('*.bin'))),12)
            for file in (Path(directory)/'settings').glob('*.did'):
                self.assertIn('snapshot_visibility',file.read_text())
                self.assertNotIn('"controllers" = opt vec',file.read_text())

    def test_log_gap_and_reset_are_not_empty_success(self):
        target=list(json.loads(acceptance.PROFILE.read_text())['roles'].values())[0]
        row=dict(target=target,method='fetch_canister_logs',ok=True,value=dict(canister_log_records=[dict(idx='10'),dict(idx='11')]))
        self.assertTrue(viewer.summarize([row],dict(logs={target:2}))['alerts'])
        self.assertTrue(viewer.summarize([row],dict(logs={target:20}))['alerts'])
        changed=viewer.summarize([row],dict(logs={target:11},tail_hashes={target:'different'}))
        self.assertIn('INVARIANT_FAILURE',[x['category'] for x in changed['alerts']])
        row['value']['canister_log_records']=[]
        self.assertTrue(viewer.summarize([row],dict(logs={target:2}))['alerts'])
        row['ok']=False
        self.assertFalse(viewer.summarize([row])['checkpoint_eligible'])

    def test_private_checkpoint_safety_and_no_provider_on_fixture(self):
        p=json.loads(acceptance.PROFILE.read_text());rows=[]
        for target in p['roles'].values():
            rows += [dict(target=target,method='canister_status',ok=True,value=dict(cycles='3',reserved_cycles='0',memory_size='1',settings={},status={},module_hash=[])),
                     dict(target=target,method='fetch_canister_logs',ok=True,value=dict(canister_log_records=[]))]
        with tempfile.TemporaryDirectory() as directory:
            out=Path(directory);file=out/'fixture.json'
            evidence=dict(caller=self.approval()['principal'],network='ic',observations=rows)
            file.write_text(json.dumps(evidence))
            with patch('viewer.collect',side_effect=AssertionError('provider must not run')):
                self.assertEqual(observe.run_viewer(acceptance.PROFILE,out,file),0)
            before=(out/'viewer-fixture-checkpoint.json').read_bytes()
            rows[-1]['ok']=False;file.write_text(json.dumps(evidence))
            self.assertEqual(observe.run_viewer(acceptance.PROFILE,out,file),2)
            self.assertEqual((out/'viewer-fixture-checkpoint.json').read_bytes(),before)
        with self.assertRaises(ValueError): viewer.summarize([dict(target='aaaaa-aa',method='canister_status')])

    def test_finite_retention_preserves_pins_partial_and_unresolved(self):
        with tempfile.TemporaryDirectory() as directory:
            out=Path(directory);now=time.time()
            for name,complete in [('raw-old.json',True),('raw-partial.json',False),('raw-pinned.json',True)]:
                observe.atomic(out/name,dict(complete=complete));os.utime(out/name,(now-100*86400,)*2)
            observe.atomic(out/'pins.json',['raw-pinned.json'])
            self.assertEqual(observe.rotate(out,int(now)),['raw-old.json'])
            self.assertTrue((out/'raw-partial.json').exists())
            self.assertTrue((out/'raw-pinned.json').exists())

    def test_archive_plan_and_initial_options_are_acceptance_only(self):
        import sns
        p=json.loads(acceptance.PROFILE.read_text())
        d=json.loads((acceptance.PROFILE.parent/'historical/2026-11-15-candidate-inputs.json').read_text())
        rendered,_,_=operational.render(p,d)
        self.assertEqual(rendered['genesis']['archive_cycles'],4_500_000_000_000)
        self.assertEqual(rendered['genesis']['archive_more_controllers'],[p['operator']])
        self.assertEqual(p['initial_main_cycles_target'],3_000_000_000_000)
        funding=json.loads((acceptance.PROFILE.parent/'funding-sheet.json').read_text())
        row=next(x for x in funding['rows'] if x['purpose']=='Ledger archive allocation before trigger')
        self.assertEqual(row['desired'],row['archive_allocation_cycles']+row['native_retained_liquid_floor']+row['existing_operational_margin_cycles'])
        self.assertEqual(row['maximum_authorized_spend'],0)


if __name__=='__main__':unittest.main()
