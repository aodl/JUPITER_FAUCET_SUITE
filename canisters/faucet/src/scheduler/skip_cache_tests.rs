// Complete-job negative-evidence tests. The oracle is a direct sum over the
// synthetic qualifying records, independent of production weighting helpers.
#[derive(Clone, Debug, Default)]
struct SkipRunMeasurements {
    calls: [u64; 3],
    records: [u64; 3],
    cursors: Vec<(bool, Option<u64>)>,
    ticks: u64,
    classifications: u64,
    lookups: u64,
    insertions: u64,
    ranges: usize,
    stable_map_bytes: u64,
}
struct SkipMeasuredIndex {
    txs: Vec<IndexTransactionWithId>,
    cap: usize,
    measurements: Mutex<SkipRunMeasurements>,
    fault: Mutex<Option<(usize, BeneficiaryPageFault)>>,
}
#[async_trait]
impl IndexClient for SkipMeasuredIndex {
    async fn get_account_identifier_transactions(
        &self,
        _: String,
        start: Option<u64>,
        max_results: u64,
    ) -> Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError> {
        assert_no_persistence_batch();
        let beneficiary = state::with_state(|st| {
            st.active_payout_job
                .as_ref()
                .is_some_and(effective_denom_scan_complete)
        });
        let mut m = self.measurements.lock().unwrap();
        let phase = state::with_state(|st| {
            st.active_payout_job
                .as_ref()
                .map(|j| usize::from(effective_denom_scan_complete(j)))
                .unwrap_or(2)
        });
        m.calls[phase] += 1;
        m.cursors.push((beneficiary, start));
        let mut txs: Vec<_> = self
            .txs
            .iter()
            .filter(|tx| start.is_none_or(|id| tx.id < id))
            .take(self.cap.min(max_results as usize))
            .cloned()
            .collect();
        let mut oldest = self.txs.last().map(|tx| tx.id);
        let mut fault = self.fault.lock().unwrap();
        if fault.as_ref().is_some_and(|(n, _)| *n == m.cursors.len()) {
            match fault.take().unwrap().1 {
                BeneficiaryPageFault::Read => {
                    return Err(crate::clients::ClientError::Call("injected read".into()))
                }
                BeneficiaryPageFault::Empty => txs.clear(),
                BeneficiaryPageFault::Duplicate => {
                    if let Some(tx) = txs.first().cloned() {
                        txs.push(tx);
                    }
                }
                BeneficiaryPageFault::Ascending => txs.reverse(),
                BeneficiaryPageFault::RepeatedCursor => {
                    if let Some(tx) = txs.first_mut() {
                        tx.id = start.unwrap();
                    }
                }
                BeneficiaryPageFault::WrongAnchor => oldest = Some(u64::MAX),
            }
        }
        m.records[phase] += txs.len() as u64;
        Ok(GetAccountIdentifierTransactionsResponse {
            balance: 300_000_000,
            transactions: txs,
            oldest_tx_id: oldest,
        })
    }
}
fn skip_reset() -> state::Config {
    state::SKIP_CACHE_TEST_STATS.with(|s| *s.borrow_mut() = Default::default());
    let mut cfg = test_config();
    cfg.staking_account.subaccount = Some([9; 32]);
    cfg.stake_recognition_delay_seconds = Some(1);
    state::set_state(state::State::new(cfg.clone(), 100));
    state::clear_skip_ranges();
    cfg
}
fn skip_fixture(count: u64, cap: usize, sparse: u64, oldest_barren: bool) -> SkipMeasuredIndex {
    let cfg = skip_reset();
    let account = account_identifier_text_for_account(&cfg.staking_account);
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")
        .unwrap()
        .to_text()
        .into_bytes();
    let mut txs = Vec::new();
    if !oldest_barren {
        txs.push(commitment_tx_at(
            0,
            &account,
            100_000_000,
            Some(target.clone()),
            0,
        ));
    }
    for i in 1..=count {
        txs.push(commitment_tx_at(i * sparse, &account, 1, None, 0));
    }
    txs.push(commitment_tx_at(
        (count + 1) * sparse,
        &account,
        100_000_000,
        Some(target),
        0,
    ));
    txs.sort_by_key(|t| std::cmp::Reverse(t.id));
    state::with_state_mut(|st| st.config.expected_first_staking_tx_id = txs.last().map(|t| t.id));
    SkipMeasuredIndex {
        txs,
        cap,
        measurements: Mutex::new(Default::default()),
        fault: Mutex::new(None),
    }
}
fn skip_start_round(index: &SkipMeasuredIndex, round: u64) {
    let end = (100 + round) * 1_000_000_000;
    state::with_state_mut(|st| {
        if st.current_round_start_time_nanos.is_none() {
            st.current_round_start_time_nanos = Some(2_000_000_000);
        }
    });
    ensure_active_job_with_boundary(
        end,
        10_000,
        100_000_000,
        999_999_999,
        end,
        Some(index.txs[0].id + 10 + round),
        Some(FundingTranche {
            tx_id: index.txs[0].id + 10 + round,
            timestamp_nanos: end,
            amount_e8s: 100_000_000,
        }),
    );
    *index.measurements.lock().unwrap() = Default::default();
    logic::TEST_COMMITMENT_CLASSIFICATIONS.with(|c| c.set(0));
    state::SKIP_CACHE_TEST_STATS.with(|c| {
        let mut s = c.borrow_mut();
        s.lookups = 0;
        s.insertions = 0;
    });
}
fn skip_finish(
    index: &SkipMeasuredIndex,
    ledger: &impl LedgerClient,
    cmc: &ScriptedCmc,
) -> SkipRunMeasurements {
    let mut ticks = 0;
    while state::with_state(|st| st.active_payout_job.is_some()) {
        assert!(ticks < 20_000, "must make bounded progress");
        run_ready(process_payout(
            ledger,
            index,
            cmc,
            &NoopGovernance,
            &crate::clients::canister_info::NoopCanisterStatusClient,
            200_000_000_000,
            200,
        ));
        ticks += 1;
        assert!(
            !state::with_state(|st| st.skip_range_invariant_fault.unwrap_or(false)),
            "unexpected cache fault"
        );
    }
    let mut m = index.measurements.lock().unwrap().clone();
    m.ticks = ticks;
    m.classifications = logic::TEST_COMMITMENT_CLASSIFICATIONS.with(|c| c.get());
    state::SKIP_CACHE_TEST_STATS.with(|c| {
        m.lookups = c.borrow().lookups;
        m.insertions = c.borrow().insertions;
    });
    m.ranges = state::list_skip_ranges().len();
    m.stable_map_bytes = state::skip_cache_test_memory_bytes();
    m
}
#[test]
fn skip_c_cold_and_warm_rounds_preserve_oracle_and_bound_interior_reads() {
    for cap in [1, 2, PAGE_SIZE as usize] {
        for count in [MIN_SKIP_RANGE_TX_COUNT, MIN_SKIP_RANGE_TX_COUNT * 3] {
            let index = skip_fixture(count, cap, 7, false);
            let mut warm = None;
            for round in 0..3 {
                skip_start_round(&index, round);
                let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(1), LedgerStep::Ok(2)]);
                let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);
                let m = skip_finish(&index, &ledger, &cmc);
                let summary = state::with_state(|st| st.last_summary.clone().unwrap());
                assert_eq!(
                    summary.effective_denom_staking_balance_e8s,
                    Some(200_000_000)
                );
                assert_eq!(summary.topped_up_count, 2);
                assert_eq!(summary.topped_up_sum_e8s, 99_980_000);
                assert_eq!(summary.remainder_to_relay_e8s, 0);
                assert_eq!(summary.pot_remaining_e8s, 0);
                assert_eq!(ledger.transfer_calls(), 2);
                assert_eq!(cmc.call_count(), 2);
                assert_eq!(
                    state::list_skip_ranges(),
                    vec![SkipRange {
                        start_tx_id: 7,
                        end_tx_id: count * 7
                    }]
                );
                assert!(
                    m.records[1] <= 2 * cap as u64 + 2,
                    "beneficiary benefits immediately: {m:?}"
                );
                if round == 0 {
                    assert_eq!(m.records[0], count + 2);
                } else {
                    assert!(m.records[0] <= 2 * cap as u64 + 2, "warm denom: {m:?}");
                    assert!(m.classifications <= 4);
                    if let Some(ref prior) = warm {
                        assert_eq!(&m.calls, prior);
                    }
                    warm = Some(m.calls);
                }
                println!("SKIP_COST cap={cap} barren={count} round={round} calls={:?} records={:?} ticks={} classifications={} lookups={} insertions={} ranges={} stable_map_bytes={}",m.calls,m.records,m.ticks,m.classifications,m.lookups,m.insertions,m.ranges,m.stable_map_bytes);
            }
        }
    }
}
#[test]
fn skip_c_threshold_counts_records_and_normalizes_descending_span() {
    for count in [
        MIN_SKIP_RANGE_TX_COUNT - 1,
        MIN_SKIP_RANGE_TX_COUNT,
        MIN_SKIP_RANGE_TX_COUNT + 1,
    ] {
        let mut span = LocalSkipCandidate::default();
        for n in (1..=count).rev() {
            span.note_skippable(n * 1000);
        }
        let result = span.finish_span();
        assert_eq!(result.is_some(), count >= MIN_SKIP_RANGE_TX_COUNT);
        if let Some(r) = result {
            assert_eq!(
                r,
                SkipRange {
                    start_tx_id: 1000,
                    end_tx_id: count * 1000
                }
            );
        }
    }
}
#[test]
fn skip_c_insertion_failure_preserves_unpaid_cursor_and_sums() {
    let index = skip_fixture(MIN_SKIP_RANGE_TX_COUNT, PAGE_SIZE as usize, 1, false);
    skip_start_round(&index, 0);
    state::SKIP_CACHE_TEST_STATS.with(|s| s.borrow_mut().fail_next_insert = true);
    let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(1), LedgerStep::Ok(2)]);
    let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);
    assert!(run_ready(process_payout(
        &ledger,
        &index,
        &cmc,
        &NoopGovernance,
        &crate::clients::canister_info::NoopCanisterStatusClient,
        200_000_000_000,
        200
    )));
    state::with_state(|st| {
        let job = st.active_payout_job.as_ref().unwrap();
        assert!(job.next_start.unwrap() > 0);
        assert!(!effective_denom_scan_complete(job));
        assert!(job.pending_transfer.is_none());
        assert_eq!(st.last_processed_funding_tx_id, None);
        assert_eq!(st.skip_range_invariant_fault, Some(true));
    });
    assert_eq!(ledger.transfer_calls(), 0);
    // Explicit local fault recovery; production fault remains sticky until operator action.
    state::with_state_mut(|st| st.skip_range_invariant_fault = Some(false));
    skip_finish(&index, &ledger, &cmc);
    assert_eq!(ledger.transfer_calls(), 2);
}
#[test]
fn skip_c_cache_at_oldest_preserves_completion_and_zero_id() {
    for cap in [1, 2, PAGE_SIZE as usize] {
        let mut index = skip_fixture(MIN_SKIP_RANGE_TX_COUNT, cap, 1, true);
        // All barren, including an ID-zero seed; no entitlement is added.
        let account = account_identifier_text_for_account(&state::with_state(|st| {
            st.config.staking_account.clone()
        }));
        index.txs[0] = commitment_tx_at(MIN_SKIP_RANGE_TX_COUNT + 1, &account, 1, None, 0);
        index.txs.push(commitment_tx_at(0, &account, 1, None, 0));
        state::with_state_mut(|st| st.config.expected_first_staking_tx_id = Some(0));
        for round in 0..2 {
            skip_start_round(&index, round);
            let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(1)]);
            let cmc = ScriptedCmc::new(vec![]);
            let m = skip_finish(&index, &ledger, &cmc);
            let s = state::with_state(|st| st.last_summary.clone().unwrap());
            assert_eq!(s.effective_denom_staking_balance_e8s, Some(0));
            assert_eq!(s.remainder_to_relay_e8s, 99_990_000);
            assert_eq!(s.topped_up_count, 0);
            if round > 0 {
                assert!(m.records[0] <= cap as u64);
                assert!(m.records[1] <= cap as u64);
            }
            assert_eq!(state::list_skip_ranges()[0].start_tx_id, 0);
        }
    }
}

#[test]
fn skip_c_endpoint_jumps_never_cross_unknown_gaps() {
    for cursor in [
        None,
        Some(250),
        Some(201),
        Some(200),
        Some(150),
        Some(100),
        Some(99),
        Some(0),
    ] {
        skip_reset();
        state::insert_skip_range(SkipRange {
            start_tx_id: 100,
            end_tx_id: 200,
        })
        .unwrap();
        let mut job = ActivePayoutJob::new(1, 10_000, 100_000_000, 100_000_000, 100_000_000_000);
        job.observed_oldest_tx_id = Some(0);
        job.next_start = cursor;
        state::with_state_mut(|st| st.active_payout_job = Some(job.clone()));
        let lease = MainLeaseToken::capture_for_test();
        let jumped = skip_cached_history(&job, lease).unwrap();
        let expected = matches!(cursor, Some(201 | 200 | 150));
        assert_eq!(jumped, expected, "cursor={cursor:?}");
        assert_eq!(
            state::with_state(|st| st.active_payout_job.as_ref().unwrap().next_start),
            if expected { Some(100) } else { cursor }
        );
    }
}

#[test]
fn skip_c_faulted_learning_never_certifies_incomplete_pages() {
    for fault in [
        BeneficiaryPageFault::Empty,
        BeneficiaryPageFault::Duplicate,
        BeneficiaryPageFault::Ascending,
        BeneficiaryPageFault::RepeatedCursor,
        BeneficiaryPageFault::WrongAnchor,
        BeneficiaryPageFault::Read,
    ] {
        let index = skip_fixture(MIN_SKIP_RANGE_TX_COUNT, 500, 1, false);
        skip_start_round(&index, 0);
        *index.fault.lock().unwrap() = Some((2, fault));
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(1), LedgerStep::Ok(2)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);
        run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance,
            &crate::clients::canister_info::NoopCanisterStatusClient,
            200_000_000_000,
            200,
        ));
        assert!(state::list_skip_ranges().is_empty());
        assert_eq!(ledger.transfer_calls(), 0);
        state::with_state(|st| {
            let job = st.active_payout_job.as_ref().unwrap();
            assert_eq!(job.next_start, Some(MIN_SKIP_RANGE_TX_COUNT + 2 - 500));
            assert_eq!(job.skip_candidate_tx_count, 499);
            assert_eq!(job.effective_denom_staking_balance_e8s, Some(100_000_000));
        });
        skip_finish(&index, &ledger, &cmc);
        assert_eq!(ledger.transfer_calls(), 2);
        assert_eq!(
            state::list_skip_ranges(),
            vec![SkipRange {
                start_tx_id: 1,
                end_tx_id: MIN_SKIP_RANGE_TX_COUNT
            }]
        );
    }
}

#[test]
fn skip_c_duplicate_overlap_and_adjacency_preserve_unknown_gaps() {
    skip_reset();
    for (lo, hi) in [(10, 20), (10, 20), (15, 30), (31, 40), (50, 60)] {
        state::insert_skip_range(SkipRange {
            start_tx_id: lo,
            end_tx_id: hi,
        })
        .unwrap();
    }
    assert_eq!(
        state::list_skip_ranges(),
        vec![
            SkipRange {
                start_tx_id: 10,
                end_tx_id: 40
            },
            SkipRange {
                start_tx_id: 50,
                end_tx_id: 60
            }
        ]
    );
    assert!(state::skip_range_containing(45).unwrap().is_none());
    state::insert_skip_range(SkipRange {
        start_tx_id: 40,
        end_tx_id: 50,
    })
    .unwrap();
    assert_eq!(
        state::list_skip_ranges(),
        vec![SkipRange {
            start_tx_id: 10,
            end_tx_id: 60
        }]
    );
    assert!(state::insert_skip_range(SkipRange {
        start_tx_id: 2,
        end_tx_id: 1
    })
    .is_err());
    // A single call never folds an unbounded map. Existing evidence is retained.
    state::clear_skip_ranges();
    for i in 0..1000 {
        state::insert_skip_range(SkipRange {
            start_tx_id: i * 10,
            end_tx_id: i * 10 + 2,
        })
        .unwrap();
    }
    state::insert_skip_range(SkipRange {
        start_tx_id: 0,
        end_tx_id: 9999,
    })
    .unwrap();
    assert_eq!(state::list_skip_ranges().len(), 1000);
}

#[test]
fn skip_c_upgrade_invalidation_includes_omitted_and_unchanged_policy() {
    for args in [
        None,
        Some(crate::UpgradeArgs::default()),
        Some(crate::UpgradeArgs {
            stake_recognition_delay_seconds: Some(86_400),
            ..Default::default()
        }),
        Some(crate::UpgradeArgs {
            stake_recognition_delay_seconds: Some(1),
            ..Default::default()
        }),
        Some(crate::UpgradeArgs {
            stake_recognition_delay_seconds: Some(604_800),
            ..Default::default()
        }),
        Some(crate::UpgradeArgs {
            min_tx_e8s: Some(200_000_000),
            ..Default::default()
        }),
    ] {
        let mut cfg = skip_reset();
        cfg.stake_recognition_delay_seconds = Some(86_400);
        cfg.staking_account.subaccount = Some([9; 32]);
        let mut st = state::State::new(cfg, 100);
        state::insert_skip_range(SkipRange {
            start_tx_id: 0,
            end_tx_id: 20_000,
        })
        .unwrap();
        crate::apply_upgrade_args_to_state(&mut st, args, 200);
        assert!(state::list_skip_ranges().is_empty());
    }
}

fn enable_skip_learning_for_test() {
    state::with_state_mut(|st| {
        let job = st.active_payout_job.as_mut().unwrap();
        job.effective_denom_scan_complete = Some(false);
        job.effective_denom_staking_balance_e8s = Some(0);
    });
}

#[test]
fn skip_extra_cached_and_uncached_interleaved_histories_match_independent_sum() {
    let mut outcomes = Vec::new();
    for disabled in [true, false] {
        let cfg = skip_reset();
        let account = account_identifier_text_for_account(&cfg.staking_account);
        let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")
            .unwrap()
            .to_text()
            .into_bytes();
        let mut txs = Vec::new();
        let mut id = 0;
        for (n, span) in [10_000, 9_999, 10_001, 0].into_iter().enumerate() {
            txs.push(commitment_tx_at(
                id,
                &account,
                (n as u64 + 1) * 100_000_000,
                Some(target.clone()),
                0,
            ));
            id += 1;
            for _ in 0..span {
                txs.push(commitment_tx_at(id, &account, 1, None, 0));
                id += 1;
            }
        }
        txs.reverse();
        state::with_state_mut(|st| st.config.expected_first_staking_tx_id = Some(0));
        let index = SkipMeasuredIndex {
            txs,
            cap: 500,
            measurements: Mutex::new(Default::default()),
            fault: Mutex::new(None),
        };
        state::SKIP_CACHE_TEST_STATS.with(|s| s.borrow_mut().disabled = disabled);
        for round in 0..3 {
            skip_start_round(&index, round);
            let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 1_000_000_000, vec![]);
            let cmc = ScriptedCmc::new(vec![CmcStep::Ok; 4]);
            let m = skip_finish(&index, &ledger, &cmc);
            // Descending payout order is 4,3,2,1; calculate each independently.
            assert_eq!(
                ledger.transfer_amounts(),
                vec![39_990_000, 29_990_000, 19_990_000, 9_990_000]
            );
            let mut summary = state::with_state(|st| st.last_summary.clone().unwrap());
            assert_eq!(
                summary.effective_denom_staking_balance_e8s,
                Some(1_000_000_000)
            );
            assert_eq!(summary.remainder_to_relay_e8s, 0);
            summary.ignored_under_threshold = 0;
            summary.ignored_bad_memo = 0;
            if disabled {
                outcomes.push(summary);
            } else {
                assert_eq!(outcomes[round as usize], summary);
                if round > 0 {
                    assert!(
                        m.records[0] < 12_000,
                        "only fragmented subthreshold span remains: {m:?}"
                    );
                }
            }
            println!(
                "SKIP_FRAGMENT disabled={disabled} round={round} calls={:?} records={:?} ranges={}",
                m.calls, m.records, m.ranges
            );
        }
        state::SKIP_CACHE_TEST_STATS.with(|s| s.borrow_mut().disabled = false);
    }
}

#[test]
fn skip_extra_appended_spam_is_learned_without_reclassifying_old_interior() {
    let mut index = skip_fixture(MIN_SKIP_RANGE_TX_COUNT, 500, 1, false);
    for round in 0..3 {
        if round == 1 {
            let account = account_identifier_text_for_account(&state::with_state(|st| {
                st.config.staking_account.clone()
            }));
            let high = index.txs[0].id;
            for n in 1..=MIN_SKIP_RANGE_TX_COUNT {
                index
                    .txs
                    .push(commitment_tx_at(high + n, &account, 1, None, 0));
            }
            index.txs.push(commitment_tx_at(
                high + MIN_SKIP_RANGE_TX_COUNT + 1,
                &account,
                100_000_000,
                Some(
                    Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")
                        .unwrap()
                        .to_text()
                        .into_bytes(),
                ),
                0,
            ));
            index.txs.sort_by_key(|t| std::cmp::Reverse(t.id));
        }
        skip_start_round(&index, round);
        let recipients = if round == 0 { 2 } else { 3 };
        let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 300_000_000, vec![]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok; recipients]);
        let m = skip_finish(&index, &ledger, &cmc);
        assert_eq!(
            ledger.transfer_amounts(),
            vec![100_000_000 / recipients as u64 - 10_000; recipients]
        );
        if round == 1 {
            assert!(
                m.classifications < MIN_SKIP_RANGE_TX_COUNT + 20,
                "old interior must not be classified"
            );
            assert_eq!(m.ranges, 2);
        }
        if round == 2 {
            assert!(m.records[0] < 1500);
            assert!(m.records[1] < 1500);
            assert_eq!(m.classifications, 6);
        }
        println!(
            "SKIP_APPEND round={round} calls={:?} records={:?} classifications={} ranges={}",
            m.calls, m.records, m.classifications, m.ranges
        );
    }
}

#[test]
fn skip_extra_policy_ineligible_only_not_zero_weight_or_delivery() {
    let mut index = skip_fixture(MIN_SKIP_RANGE_TX_COUNT, 500, 1, false);
    let account = account_identifier_text_for_account(&state::with_state(|st| {
        st.config.staking_account.clone()
    }));
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")
        .unwrap()
        .to_text();
    // Three valid directives at future effective times split the barren span.
    let ids = [2500, 5000, 7500];
    for (id, memo) in ids.into_iter().zip([
        target.clone().into_bytes(),
        format!("{}.raw", target.replace('-', "")).into_bytes(),
        b"11614578985374291210".to_vec(),
    ]) {
        let tx = index.txs.iter_mut().find(|t| t.id == id).unwrap();
        *tx = commitment_tx_at(id, &account, 100_000_000, Some(memo), 300_000_000_000);
    }
    skip_start_round(&index, 0);
    let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(1), LedgerStep::Ok(2)]);
    let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);
    skip_finish(&index, &ledger, &cmc);
    for id in ids {
        assert!(state::skip_range_containing(id).unwrap().is_none());
    }
    assert!(
        state::list_skip_ranges().is_empty(),
        "subthreshold fragments separated by valid zero-weight commitments"
    );
    assert_eq!(
        state::with_state(|st| st
            .last_summary
            .as_ref()
            .unwrap()
            .effective_denom_staking_balance_e8s),
        Some(200_000_000)
    );
}

struct SkipManyIntervalsIndex;
#[async_trait]
impl IndexClient for SkipManyIntervalsIndex {
    async fn get_account_identifier_transactions(
        &self,
        _: String,
        start: Option<u64>,
        _: u64,
    ) -> Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError> {
        // Implicit dense history avoids allocating millions of fixture records.
        let id = start.unwrap_or(2_000_000).saturating_sub(1);
        Ok(GetAccountIdentifierTransactionsResponse {
            balance: 1,
            oldest_tx_id: Some(0),
            transactions: vec![commitment_tx(id, "", 1, None)],
        })
    }
}
#[test]
fn skip_extra_many_jumps_are_bounded_without_whole_map_loading() {
    skip_reset();
    // Each interval stands for 10,000 previously examined records. Single-record
    // unverified gaps force alternating page reads and jumps.
    for n in 0..190u64 {
        state::insert_skip_range(SkipRange {
            start_tx_id: n * 10001,
            end_tx_id: n * 10001 + 9999,
        })
        .unwrap();
    }
    let mut job = ActivePayoutJob::new(100, 10_000, 100_000_000, 100_000_000, 200_000_000_000);
    job.next_start = Some(1_900_189);
    job.observed_oldest_tx_id = Some(0);
    job.configure_round_accounting(
        Some(1),
        None,
        200_000_000_000,
        Some(2_000_000),
        0,
        false,
    );
    job.next_start = Some(1_900_189);
    job.observed_oldest_tx_id = Some(0);
    state::with_state_mut(|st| st.active_payout_job = Some(job.clone()));
    let ledger = ScriptedLedger::new(vec![]);
    let cmc = ScriptedCmc::new(vec![]);
    assert!(run_ready(process_payout(
        &ledger,
        &SkipManyIntervalsIndex,
        &cmc,
        &NoopGovernance,
        &crate::clients::canister_info::NoopCanisterStatusClient,
        200_000_000_000,
        200
    )));
    let after = state::with_state(|st| st.active_payout_job.clone().unwrap());
    assert!(!effective_denom_scan_complete(&after));
    assert!(after.next_start < job.next_start);
    assert!(after.next_start.unwrap() > 1_000_000);
    let stats = state::SKIP_CACHE_TEST_STATS.with(|s| s.borrow().clone());
    assert!(
        stats.lookups <= 128,
        "bounded page/jump operations: {stats:?}"
    );
    assert_eq!(ledger.transfer_calls(), 0);
    println!(
        "SKIP_MANY ranges={} cursor_before={:?} cursor_after={:?} stats={stats:?} stable_bytes={}",
        state::list_skip_ranges().len(),
        job.next_start,
        after.next_start,
        state::skip_cache_test_memory_bytes()
    );
}

struct SkipHeldLearningPage<'a> {
    inner: &'a SkipMeasuredIndex,
    release: AtomicBool,
}
#[async_trait]
impl IndexClient for SkipHeldLearningPage<'_> {
    async fn get_account_identifier_transactions(
        &self,
        account: String,
        start: Option<u64>,
        max: u64,
    ) -> Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError> {
        let response = self
            .inner
            .get_account_identifier_transactions(account, start, max)
            .await;
        if self.inner.measurements.lock().unwrap().cursors.len() == 21 {
            std::future::poll_fn(|_| {
                if self.release.load(Ordering::SeqCst) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
        }
        response
    }
}
#[test]
fn skip_extra_stale_learning_page_cannot_persist_exclusions_or_sums() {
    for mode in 0..4 {
        let index = skip_fixture(MIN_SKIP_RANGE_TX_COUNT, 500, 1, false);
        skip_start_round(&index, 0);
        let held = SkipHeldLearningPage {
            inner: &index,
            release: AtomicBool::new(false),
        };
        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![]);
        let status = crate::clients::canister_info::NoopCanisterStatusClient;
        let mut future = Box::pin(process_payout(
            &ledger,
            &held,
            &cmc,
            &NoopGovernance,
            &status,
            200_000_000_000,
            200,
        ));
        assert!(poll_once(future.as_mut()).is_pending());
        assert!(state::list_skip_ranges().is_empty());
        state::with_state_mut(|st| {
            let j = st.active_payout_job.as_mut().unwrap();
            match mode {
                0 => st.main_lock_state_ts = Some(999),
                1 => j.id += 1,
                2 => j.next_start = Some(999),
                _ => j.effective_denom_scan_complete = Some(true),
            }
        });
        let before =
            state::with_state(|st| candid::encode_one(st.active_payout_job.clone()).unwrap());
        held.release.store(true, Ordering::SeqCst);
        assert!(poll_once(future.as_mut()).is_ready());
        assert_eq!(
            state::with_state(|st| candid::encode_one(st.active_payout_job.clone()).unwrap()),
            before
        );
        assert!(state::list_skip_ranges().is_empty());
        assert_eq!(ledger.transfer_calls(), 0);
    }
}

#[test]
fn skip_extra_definitive_delivery_failure_does_not_change_exclusions() {
    let index = skip_fixture(MIN_SKIP_RANGE_TX_COUNT, 500, 1, false);
    for failed in [true, false] {
        skip_start_round(&index, u64::from(!failed));
        let ledger = if failed {
            ScriptedLedger::new(vec![
                LedgerStep::PermanentErr,
                LedgerStep::PermanentErr,
                LedgerStep::PermanentErr,
                LedgerStep::PermanentErr,
                LedgerStep::Ok(3),
            ])
        } else {
            ScriptedLedger::new(vec![LedgerStep::Ok(4), LedgerStep::Ok(5)])
        };
        let cmc = ScriptedCmc::new(if failed {
            vec![]
        } else {
            vec![CmcStep::Ok, CmcStep::Ok]
        });
        skip_finish(&index, &ledger, &cmc);
        let s = state::with_state(|st| st.last_summary.clone().unwrap());
        assert_eq!(s.effective_denom_staking_balance_e8s, Some(200_000_000));
        assert_eq!(s.topped_up_count, if failed { 0 } else { 2 });
        assert_eq!(
            state::list_skip_ranges(),
            vec![SkipRange {
                start_tx_id: 1,
                end_tx_id: MIN_SKIP_RANGE_TX_COUNT
            }]
        );
        assert!(state::skip_range_containing(0).unwrap().is_none());
        assert!(state::skip_range_containing(MIN_SKIP_RANGE_TX_COUNT + 1)
            .unwrap()
            .is_none());
    }
}

#[test]
fn skip_extra_threshold_change_reclassifies_previously_excluded_commitments() {
    let index = skip_fixture(MIN_SKIP_RANGE_TX_COUNT, 500, 1, false);
    state::with_state_mut(|st| st.config.min_tx_e8s = 200_000_000);
    skip_start_round(&index, 0);
    let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(1)]);
    let cmc = ScriptedCmc::new(vec![]);
    skip_finish(&index, &ledger, &cmc);
    assert!(state::skip_range_containing(0).unwrap().is_some());
    state::with_state_mut(|st| {
        st.config.stake_recognition_delay_seconds = Some(1);
        crate::apply_upgrade_args_to_state(
            st,
            Some(crate::UpgradeArgs {
                min_tx_e8s: Some(100_000_000),
                ..Default::default()
            }),
            200,
        );
    });
    assert!(state::list_skip_ranges().is_empty());
    skip_start_round(&index, 1);
    let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(2), LedgerStep::Ok(3)]);
    let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);
    skip_finish(&index, &ledger, &cmc);
    assert_eq!(cmc.call_count(), 2);
    assert!(state::skip_range_containing(0).unwrap().is_none());
}

struct SkipHeldNeuronLookup;
#[async_trait]
impl GovernanceClient for SkipHeldNeuronLookup {
    async fn neuron_staking_subaccount(
        &self,
        _: u64,
    ) -> Result<[u8; 32], crate::clients::ClientError> {
        pending().await
    }
    async fn claim_or_refresh_neuron(&self, _: u64) -> Result<(), crate::clients::ClientError> {
        panic!("lookup has not completed")
    }
}
#[test]
fn skip_extra_interrupted_neuron_lookup_preserves_unpaid_record_after_cached_span() {
    let mut index = skip_fixture(MIN_SKIP_RANGE_TX_COUNT, 500, 1, false);
    index.txs.last_mut().unwrap().transaction.icrc1_memo = Some(b"11614578985374291210".to_vec());
    skip_start_round(&index, 0);
    let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(1), LedgerStep::Ok(2)]);
    let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);
    let status = crate::clients::canister_info::NoopCanisterStatusClient;
    let mut future = Box::pin(process_payout(
        &ledger,
        &index,
        &cmc,
        &SkipHeldNeuronLookup,
        &status,
        200_000_000_000,
        200,
    ));
    assert!(poll_once(future.as_mut()).is_pending());
    drop(future);
    assert_eq!(ledger.transfer_calls(), 1);
    state::with_state(|st| {
        let j = st.active_payout_job.as_ref().unwrap();
        assert_eq!(j.next_start, Some(1));
        assert!(j.pending_transfer.is_none());
        assert_eq!(j.gross_outflow_e8s, 50_000_000);
    });
    let governance = ScriptedGovernance::new(vec![Ok([7; 32])]);
    assert!(run_ready(process_payout(
        &ledger,
        &index,
        &cmc,
        &governance,
        &status,
        200_000_000_000,
        200
    )));
    assert_eq!(ledger.transfer_calls(), 2);
    assert_eq!(cmc.call_count(), 1);
    assert_eq!(governance.calls(), vec![11_614_578_985_374_291_210]);
    assert_eq!(
        state::with_state(|st| st.last_summary.as_ref().unwrap().remainder_to_relay_e8s),
        0
    );
}

#[test]
fn skip_extra_noncommitment_operations_are_negative_evidence() {
    let mut index = skip_fixture(MIN_SKIP_RANGE_TX_COUNT, 500, 1, false);
    let account = account_identifier_text_for_account(&state::with_state(|st| {
        st.config.staking_account.clone()
    }));
    for tx in index
        .txs
        .iter_mut()
        .filter(|t| t.id > 0 && t.id <= MIN_SKIP_RANGE_TX_COUNT)
    {
        tx.transaction.operation = match tx.id % 3 {
            0 => IndexOperation::Mint {
                to: account.clone(),
                amount: Tokens::new(1),
            },
            1 => IndexOperation::Burn {
                from: account.clone(),
                amount: Tokens::new(1),
                spender: None,
            },
            _ => IndexOperation::Transfer {
                from: account.clone(),
                to: "other".into(),
                amount: Tokens::new(1),
                fee: Tokens::new(10_000),
                spender: None,
            },
        };
    }
    skip_start_round(&index, 0);
    let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(1), LedgerStep::Ok(2)]);
    let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);
    skip_finish(&index, &ledger, &cmc);
    assert_eq!(
        state::list_skip_ranges(),
        vec![SkipRange {
            start_tx_id: 1,
            end_tx_id: MIN_SKIP_RANGE_TX_COUNT
        }]
    );
    assert_eq!(
        state::with_state(|st| st
            .last_summary
            .as_ref()
            .unwrap()
            .effective_denom_staking_balance_e8s),
        Some(200_000_000)
    );
}
#[test]
fn skip_extra_funding_boundary_inside_cached_span_keeps_both_sides_reachable() {
    for cap in [1, 2, 500] {
        let index = skip_fixture(MIN_SKIP_RANGE_TX_COUNT, cap, 1, false);
        // Independently validated fixture evidence; all 10,000 covered records are nonqualifying.
        state::insert_skip_range(SkipRange {
            start_tx_id: 1,
            end_tx_id: MIN_SKIP_RANGE_TX_COUNT,
        })
        .unwrap();
        skip_start_round(&index, 0);
        state::with_state_mut(|st| {
            st.active_payout_job
                .as_mut()
                .unwrap()
                .round_end_latest_tx_id = Some(MIN_SKIP_RANGE_TX_COUNT / 2)
        });
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(1)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);
        let m = skip_finish(&index, &ledger, &cmc);
        assert_eq!(ledger.amounts(), vec![99_990_000]);
        assert_eq!(
            state::with_state(|st| st
                .last_summary
                .as_ref()
                .unwrap()
                .effective_denom_staking_balance_e8s),
            Some(100_000_000)
        );
        assert!(m.records[0] <= 2 * cap as u64 + 2);
        skip_start_round(&index, 1);
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(2), LedgerStep::Ok(3)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);
        skip_finish(&index, &ledger, &cmc);
        assert_eq!(ledger.amounts(), vec![49_990_000; 2]);
    }
}
#[test]
fn skip_extra_extreme_intervals_do_not_wrap() {
    skip_reset();
    state::insert_skip_range(SkipRange {
        start_tx_id: 0,
        end_tx_id: 0,
    })
    .unwrap();
    state::insert_skip_range(SkipRange {
        start_tx_id: u64::MAX,
        end_tx_id: u64::MAX,
    })
    .unwrap();
    assert!(state::skip_range_containing(1).unwrap().is_none());
    assert_eq!(state::list_skip_ranges().len(), 2);
    state::insert_skip_range(SkipRange {
        start_tx_id: 1,
        end_tx_id: u64::MAX - 1,
    })
    .unwrap();
    assert_eq!(
        state::list_skip_ranges(),
        vec![SkipRange {
            start_tx_id: 0,
            end_tx_id: u64::MAX
        }]
    );
}

// No qualifying separator: the newly examined span ends at a cached jump.
fn skip_jump_append(index: &mut SkipMeasuredIndex, count: u64, stride: u64) {
    let account = account_identifier_text_for_account(&state::with_state(|st| {
        st.config.staking_account.clone()
    }));
    let high = index.txs[0].id;
    for n in 1..=count {
        index
            .txs
            .push(commitment_tx_at(high + n * stride, &account, 1, None, 0));
    }
    index.txs.sort_by_key(|t| std::cmp::Reverse(t.id));
}
fn skip_jump_paid_round(index: &SkipMeasuredIndex, round: u64) -> SkipRunMeasurements {
    skip_start_round(index, round);
    let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 300_000_000, vec![]);
    let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);
    let m = skip_finish(index, &ledger, &cmc);
    let summary = state::with_state(|st| st.last_summary.clone().unwrap());
    assert_eq!(
        summary.effective_denom_staking_balance_e8s,
        Some(100_000_000)
    );
    assert_eq!(summary.topped_up_count, 1);
    assert_eq!(summary.topped_up_sum_e8s, 99_990_000);
    assert_eq!(summary.remainder_to_relay_e8s, 0);
    assert_eq!(summary.pot_remaining_e8s, 0);
    assert_eq!(ledger.transfer_amounts(), vec![99_990_000]);
    let cmc_id = state::with_state(|st| st.config.cmc_canister_id);
    assert_eq!(
        ledger.transfer_args.lock().unwrap()[0].to,
        logic::cmc_deposit_account(
            cmc_id,
            Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap()
        )
    );
    assert_eq!(cmc.call_count(), 1);
    m
}
#[test]
fn skip_jump_unseparated_append_native_reproduction() {
    let mut missing = Vec::new();
    for cap in [1, 2, 500] {
        let mut index = skip_fixture(MIN_SKIP_RANGE_TX_COUNT, cap, 1, false);
        index.txs.remove(0); // Only block zero qualifies; no separator above spam.
        super::super::scan::SKIP_JUMP_OBSERVATIONS.with(|o| *o.borrow_mut() = Some(Vec::new()));
        for round in 0..3 {
            if round == 1 {
                skip_jump_append(&mut index, MIN_SKIP_RANGE_TX_COUNT, 1);
            }
            let m = skip_jump_paid_round(&index, round);
            let observations = super::super::scan::SKIP_JUMP_OBSERVATIONS
                .with(|o| std::mem::take(o.borrow_mut().as_mut().unwrap()));
            for j in observations
                .iter()
                .filter(|j| !effective_denom_scan_complete(j))
            {
                println!("JUMP_BEFORE cap={cap} round={round} cursor={:?} partial_high={:?} partial_low={:?} count={} denom={:?}", j.next_start,j.skip_candidate_start_tx_id,j.skip_candidate_end_tx_id,j.skip_candidate_tx_count,j.effective_denom_staking_balance_e8s);
            }
            let ranges = state::list_skip_ranges();
            println!("JUMP_ROUND cap={cap} round={round} calls={:?} records={:?} classifications={} ranges={ranges:?}",m.calls,m.records,m.classifications);
            if round == 0 {
                assert_eq!(
                    ranges,
                    vec![SkipRange {
                        start_tx_id: 1,
                        end_tx_id: 10000
                    }]
                );
            } else {
                if round == 1 {
                    let j = observations
                        .iter()
                        .find(|j| !effective_denom_scan_complete(j))
                        .unwrap();
                    assert_eq!(j.next_start, Some(10001));
                    assert_eq!(j.skip_candidate_start_tx_id, Some(20000));
                    assert_eq!(j.skip_candidate_end_tx_id, Some(10001));
                    assert_eq!(j.skip_candidate_tx_count, 10000);
                }
                if round == 2 && ranges.iter().any(|r| r.end_tx_id >= 20000) {
                    assert_eq!(&m.calls[..2], &[2, 2]);
                    assert_eq!(&m.records[..2], &[cap as u64 + 1, cap as u64 + 1]);
                    assert_eq!(m.classifications, 2);
                }
                if !ranges
                    .iter()
                    .any(|r| r.start_tx_id <= 10001 && r.end_tx_id >= 20000)
                {
                    missing.push((cap, round, m.records, m.classifications));
                }
            }
        }
        super::super::scan::SKIP_JUMP_OBSERVATIONS.with(|o| *o.borrow_mut() = None);
    }
    assert!(missing.is_empty(), "completed evidence lost: {missing:?}");
}

#[test]
fn skip_jump_threshold_alignment_episodes_and_uncached_reference() {
    for cap in [2, 500] {
        for stride in [1, 7] {
            for count in [9999, 10000, 10001] {
                if cap == 2 && (stride != 1 || count != 10001) {
                    continue;
                }
                let mut index = skip_fixture(10000, cap, stride, false);
                index.txs.remove(0);
                skip_jump_paid_round(&index, 0);
                for episode in 0..2 {
                    let previous_high = index.txs[0].id;
                    skip_jump_append(&mut index, count, stride);
                    let high = index.txs[0].id;
                    let cold = skip_jump_paid_round(&index, 1 + episode * 3);
                    let ranges = state::list_skip_ranges();
                    assert!(ranges
                        .iter()
                        .all(|r| r.start_tx_id > 0 && r.start_tx_id <= r.end_tx_id));
                    let represented = ranges
                        .iter()
                        .any(|r| r.start_tx_id <= previous_high + stride && r.end_tx_id >= high);
                    assert_eq!(represented, count >= 10000 || episode > 0, "cap={cap} stride={stride} count={count} episode={episode} ranges={ranges:?}");
                    let warm = skip_jump_paid_round(&index, 2 + episode * 3);
                    if stride > 1 && count >= 10000 {
                        assert!(
                            state::skip_range_containing(previous_high + 1)
                                .unwrap()
                                .is_none(),
                            "sparse endpoint gap must not be unioned"
                        );
                    }
                    // A second subthreshold episode also crosses the threshold:
                    // the earlier uncached records are examined again, contiguously.
                    if count >= 10000 || episode > 0 {
                        assert!(warm.records[0] <= 3 * cap as u64 + 1, "{warm:?}");
                        assert!(warm.records[1] <= 3 * cap as u64 + 1, "{warm:?}");
                        assert_eq!(warm.classifications, 2);
                    }
                    // Same history, policy and a fully recognised commitment at every
                    // boundary: disabling cache must produce the same exact payment.
                    state::SKIP_CACHE_TEST_STATS.with(|s| s.borrow_mut().disabled = true);
                    let reference = skip_jump_paid_round(&index, 3 + episode * 3);
                    assert_eq!(reference.records[0], index.txs.len() as u64);
                    assert_eq!(reference.records[1], index.txs.len() as u64);
                    state::SKIP_CACHE_TEST_STATS.with(|s| s.borrow_mut().disabled = false);
                    println!("JUMP_MATRIX cap={cap} stride={stride} count={count} episode={episode} cold={:?}/{:?}/{} warm={:?}/{:?}/{} reference={:?}/{:?}/{} ranges={ranges:?}",cold.calls,cold.records,cold.classifications,warm.calls,warm.records,warm.classifications,reference.calls,reference.records,reference.classifications);
                }
            }
        }
    }
}

#[test]
fn skip_jump_checkpoint_failure_and_retry_preserve_learning_and_payment() {
    // 64 pages end exactly at cursor 10001. The jump is the next tick's first step.
    let mut index = skip_fixture(10000, 500, 1, false);
    index.txs.remove(0);
    skip_jump_paid_round(&index, 0);
    skip_jump_append(&mut index, 32000, 1);
    skip_start_round(&index, 1);
    let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 300_000_000, vec![]);
    let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);
    run_ready(process_payout(
        &ledger,
        &index,
        &cmc,
        &NoopGovernance,
        &crate::clients::canister_info::NoopCanisterStatusClient,
        200_000_000_000,
        200,
    ));
    let job = state::with_state(|st| st.active_payout_job.clone().unwrap());
    assert_eq!(job.next_start, Some(10001));
    assert_eq!(job.skip_candidate_start_tx_id, Some(42000));
    assert_eq!(job.skip_candidate_end_tx_id, Some(10001));
    assert_eq!(job.skip_candidate_tx_count, 32000);
    assert_eq!(index.measurements.lock().unwrap().calls[0], 64);
    assert_eq!(index.measurements.lock().unwrap().records[0], 32000);
    let before = candid::encode_one(&job).unwrap();
    state::SKIP_CACHE_TEST_STATS.with(|s| s.borrow_mut().fail_next_insert = true);
    run_ready(process_payout(
        &ledger,
        &index,
        &cmc,
        &NoopGovernance,
        &crate::clients::canister_info::NoopCanisterStatusClient,
        200_000_000_000,
        200,
    ));
    assert_eq!(
        state::with_state(|st| candid::encode_one(st.active_payout_job.as_ref().unwrap()).unwrap()),
        before
    );
    assert_eq!(
        state::with_state(|st| st.skip_range_invariant_fault),
        Some(true)
    );
    assert_eq!(
        state::list_skip_ranges(),
        vec![SkipRange {
            start_tx_id: 1,
            end_tx_id: 10000
        }]
    );
    assert!(ledger.transfer_amounts().is_empty());
    assert_eq!(cmc.call_count(), 0);
    state::with_state_mut(|st| st.skip_range_invariant_fault = Some(false));
    let m = skip_finish(&index, &ledger, &cmc);
    assert_eq!(m.records[0], 32001); // Retry reads only the qualifying oldest anchor.
    assert_eq!(ledger.transfer_amounts(), vec![99_990_000]);
    assert_eq!(cmc.call_count(), 1);
    assert_eq!(
        state::with_state(|st| st.last_summary.as_ref().unwrap().remainder_to_relay_e8s),
        0
    );
    let warm = skip_jump_paid_round(&index, 2);
    assert_eq!(&warm.records[..2], &[501, 501]);
    assert_eq!(warm.classifications, 2);
    println!("JUMP_CHECKPOINT before_cursor={:?} partial={:?}/{:?}/{} retry_calls={:?} retry_records={:?} warm_records={:?}",job.next_start,job.skip_candidate_start_tx_id,job.skip_candidate_end_tx_id,job.skip_candidate_tx_count,m.calls,m.records,warm.records);
}

#[test]
fn skip_jump_fences_persistence_and_cursor_for_superseded_owners() {
    for mode in 0..6 {
        skip_reset();
        state::insert_skip_range(SkipRange {
            start_tx_id: 1,
            end_tx_id: 10000,
        })
        .unwrap();
        let mut job = ActivePayoutJob::new(1, 10_000, 100_000_000, 100_000_000, 100_000_000_000);
        job.observed_oldest_tx_id = Some(0);
        job.next_start = Some(10001);
        job.effective_denom_scan_complete = Some(false);
        job.skip_candidate_start_tx_id = Some(20000);
        job.skip_candidate_end_tx_id = Some(10001);
        job.skip_candidate_tx_count = 10000;
        state::with_state_mut(|st| st.active_payout_job = Some(job.clone()));
        let lease = MainLeaseToken::capture_for_test();
        state::with_state_mut(|st| {
            let active = st.active_payout_job.as_mut().unwrap();
            match mode {
                0 => st.main_lock_state_ts = Some(999),
                1 => active.id += 1,
                2 => active.next_start = Some(10002),
                3 => active.effective_denom_scan_complete = Some(true),
                4 => active.scan_complete = true,
                _ => {
                    active.pending_transfer = Some(PendingTransfer {
                        notification: PendingNotification {
                            kind: TransferKind::Beneficiary,
                            beneficiary: Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")
                                .unwrap(),
                            gross_share_e8s: 100_000_000,
                            amount_e8s: 99_990_000,
                            block_index: 7,
                            next_start: Some(0),
                            transfer_memo: None,
                            destination_subaccount: None,
                            neuron_id: None,
                        },
                        created_at_time_nanos: 100_000_000_000,
                        phase: PendingTransferPhase::TransferAccepted,
                    })
                }
            }
        });
        let before =
            state::with_state(|st| candid::encode_one(st.active_payout_job.clone()).unwrap());
        let insertions = state::SKIP_CACHE_TEST_STATS.with(|s| s.borrow().insertions);
        assert!(!skip_cached_history(&job, lease).unwrap());
        assert_eq!(
            state::with_state(|st| candid::encode_one(st.active_payout_job.clone()).unwrap()),
            before
        );
        assert_eq!(
            state::SKIP_CACHE_TEST_STATS.with(|s| s.borrow().insertions),
            insertions
        );
        assert_eq!(
            state::list_skip_ranges(),
            vec![SkipRange {
                start_tx_id: 1,
                end_tx_id: 10000
            }]
        );
    }
}

struct SkipJumpHeldPage<'a> {
    inner: &'a SkipMeasuredIndex,
    release: AtomicBool,
}
#[async_trait]
impl IndexClient for SkipJumpHeldPage<'_> {
    async fn get_account_identifier_transactions(
        &self,
        account: String,
        start: Option<u64>,
        max: u64,
    ) -> Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError> {
        let response = self
            .inner
            .get_account_identifier_transactions(account, start, max)
            .await;
        if start == Some(10501) {
            std::future::poll_fn(|_| {
                if self.release.load(Ordering::SeqCst) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
        }
        response
    }
}
#[test]
fn skip_jump_held_final_learning_page_rejects_superseded_ownership() {
    for supersede in [false, true] {
        let mut index = skip_fixture(10000, 500, 1, false);
        index.txs.remove(0);
        skip_jump_paid_round(&index, 0);
        skip_jump_append(&mut index, 10000, 1);
        skip_start_round(&index, 1);
        let held = SkipJumpHeldPage {
            inner: &index,
            release: AtomicBool::new(false),
        };
        let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 300_000_000, vec![]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);
        let status = crate::clients::canister_info::NoopCanisterStatusClient;
        let mut future = Box::pin(process_payout(
            &ledger,
            &held,
            &cmc,
            &NoopGovernance,
            &status,
            200_000_000_000,
            200,
        ));
        assert!(poll_once(future.as_mut()).is_pending());
        state::with_state(|st| {
            let j = st.active_payout_job.as_ref().unwrap();
            assert_eq!(j.next_start, Some(10501));
            assert_eq!(j.skip_candidate_tx_count, 9500);
        });
        assert_eq!(
            state::list_skip_ranges(),
            vec![SkipRange {
                start_tx_id: 1,
                end_tx_id: 10000
            }]
        );
        assert!(ledger.transfer_amounts().is_empty());
        if supersede {
            state::with_state_mut(|st| st.main_lock_state_ts = Some(999));
        }
        let before =
            state::with_state(|st| candid::encode_one(st.active_payout_job.clone()).unwrap());
        held.release.store(true, Ordering::SeqCst);
        assert!(poll_once(future.as_mut()).is_ready());
        drop(future);
        if supersede {
            assert_eq!(
                state::with_state(|st| candid::encode_one(st.active_payout_job.clone()).unwrap()),
                before
            );
            assert_eq!(
                state::list_skip_ranges(),
                vec![SkipRange {
                    start_tx_id: 1,
                    end_tx_id: 10000
                }]
            );
            assert!(ledger.transfer_amounts().is_empty());
            skip_finish(&index, &ledger, &cmc);
        }
        assert_eq!(ledger.transfer_amounts(), vec![99_990_000]);
        assert_eq!(cmc.call_count(), 1);
        assert_eq!(
            state::list_skip_ranges(),
            vec![SkipRange {
                start_tx_id: 1,
                end_tx_id: 20000
            }]
        );
    }
}
#[test]
fn skip_jump_oldest_completion_requires_persistence_and_beneficiary_never_learns() {
    for beneficiary in [false, true] {
        skip_reset();
        state::insert_skip_range(SkipRange {
            start_tx_id: 0,
            end_tx_id: 10000,
        })
        .unwrap();
        let mut job = ActivePayoutJob::new(1, 10_000, 100_000_000, 100_000_000, 100_000_000_000);
        job.observed_oldest_tx_id = Some(0);
        job.next_start = Some(10001);
        job.effective_denom_scan_complete = Some(beneficiary);
        job.skip_candidate_start_tx_id = Some(20000);
        job.skip_candidate_end_tx_id = Some(10001);
        job.skip_candidate_tx_count = 10000;
        state::with_state_mut(|st| st.active_payout_job = Some(job.clone()));
        let lease = MainLeaseToken::capture_for_test();
        state::SKIP_CACHE_TEST_STATS.with(|s| s.borrow_mut().fail_next_insert = true);
        if !beneficiary {
            assert!(skip_cached_history(&job, lease).is_err());
            assert_eq!(
                state::with_state(
                    |st| candid::encode_one(st.active_payout_job.as_ref().unwrap()).unwrap()
                ),
                candid::encode_one(&job).unwrap()
            );
        }
        assert!(skip_cached_history(&job, lease).unwrap());
        state::with_state(|st| {
            let j = st.active_payout_job.as_ref().unwrap();
            assert!(effective_denom_scan_complete(j));
            assert_eq!(j.scan_complete, beneficiary);
            assert_eq!(j.next_start, if beneficiary { Some(0) } else { None });
            assert_eq!(j.skip_candidate_tx_count, 0);
        });
        assert_eq!(
            state::list_skip_ranges(),
            vec![SkipRange {
                start_tx_id: 0,
                end_tx_id: if beneficiary { 10000 } else { 20000 }
            }]
        );
        assert_eq!(
            state::SKIP_CACHE_TEST_STATS.with(|s| s.borrow().fail_next_insert),
            beneficiary
        );
    }
}
