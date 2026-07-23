use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn truncate_error(msg: &str, max_chars: usize) -> String {
    let flat = msg.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max_chars {
        flat
    } else {
        let mut out = flat.chars().take(max_chars).collect::<String>();
        out.push_str("...");
        out
    }
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if matches!(chars.peek(), Some('[')) {
                let _ = chars.next();
                while let Some(c) = chars.next() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
                continue;
            }
        }
        out.push(ch);
    }
    out
}

fn should_live_print_rust_test_line(line: &str) -> bool {
    let stripped = strip_ansi(line);
    let trimmed = stripped.trim();
    trimmed.starts_with("running ")
        || trimmed.starts_with("test ")
        || trimmed == "failures:"
        || trimmed.starts_with("test result:")
        || trimmed.starts_with("error[")
        || trimmed.starts_with("error:")
        || trimmed.starts_with("warning:")
}

fn parse_failed_rust_test_details(output: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = output.lines().collect();
    let mut blocks: BTreeMap<String, String> = BTreeMap::new();
    let mut i = 0usize;

    while i < lines.len() {
        let stripped = strip_ansi(lines[i]);
        let trimmed = stripped.trim();
        if let Some(name) = trimmed
            .strip_prefix("---- ")
            .and_then(|s| s.strip_suffix(" stdout ----"))
            .or_else(|| {
                trimmed
                    .strip_prefix("---- ")
                    .and_then(|s| s.strip_suffix(" stderr ----"))
            })
        {
            let test_name = name.to_string();
            i += 1;
            let mut body: Vec<String> = Vec::new();
            while i < lines.len() {
                let inner_stripped = strip_ansi(lines[i]);
                let inner = inner_stripped.trim();
                let starts_next = inner.starts_with("---- ")
                    && (inner.ends_with(" stdout ----") || inner.ends_with(" stderr ----"));
                if starts_next || inner == "failures:" || inner.starts_with("test result:") {
                    break;
                }
                body.push(strip_ansi(lines[i]));
                i += 1;
            }
            let body_str = body.join("\n").trim().to_string();
            blocks.entry(test_name).or_insert(body_str);
            continue;
        }
        i += 1;
    }

    let mut ordered_names: BTreeSet<String> = BTreeSet::new();
    for line in output.lines() {
        let stripped = strip_ansi(line);
        let trimmed = stripped.trim();
        if let Some(rest) = trimmed.strip_prefix("test ") {
            if let Some(name) = rest.strip_suffix(" ... FAILED") {
                ordered_names.insert(name.to_string());
            }
        }
    }
    for name in blocks.keys() {
        ordered_names.insert(name.clone());
    }

    ordered_names
        .into_iter()
        .map(|name| {
            let detail = blocks
                .get(&name)
                .cloned()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "see cargo output above".to_string());
            (name, detail)
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct LibtestSummary {
    passed: usize,
    failed: usize,
    ignored: usize,
    measured: usize,
}

fn parse_libtest_summary(output: &str) -> Option<LibtestSummary> {
    let mut summary = None;
    for line in output.lines() {
        let stripped = strip_ansi(line);
        let trimmed = stripped.trim();
        if let Some(rest) = trimmed.strip_prefix("test result:") {
            let mut parsed = LibtestSummary::default();
            let mut saw_part = false;
            for part in rest.split(';') {
                let words = part.split_whitespace().collect::<Vec<_>>();
                for pair in words.windows(2) {
                    let Some(count) = pair[0].parse::<usize>().ok() else {
                        continue;
                    };
                    match pair[1] {
                        "passed" => {
                            parsed.passed = count;
                            saw_part = true;
                        }
                        "failed" => {
                            parsed.failed = count;
                            saw_part = true;
                        }
                        "ignored" => {
                            parsed.ignored = count;
                            saw_part = true;
                        }
                        "measured" => {
                            parsed.measured = count;
                            saw_part = true;
                        }
                        _ => {}
                    }
                }
            }
            if saw_part {
                summary = Some(parsed);
            }
        }
    }
    summary
}

fn exact_execution_error(summary: Option<LibtestSummary>, status_success: bool) -> Option<String> {
    let Some(summary) = summary else {
        return if status_success {
            Some(
                "expected exactly one passed test, but libtest did not report a summary"
                    .to_string(),
            )
        } else {
            None
        };
    };
    if summary.passed == 1 && summary.failed == 0 && summary.ignored == 0 && summary.measured == 0 {
        None
    } else if summary.passed == 0
        && summary.failed == 0
        && summary.ignored == 0
        && summary.measured == 0
    {
        Some(
            "expected exactly one passed test, but libtest reported zero matched tests".to_string(),
        )
    } else {
        Some(format!(
            "expected exactly one passed test, got {} passed; {} failed; {} ignored; {} measured",
            summary.passed, summary.failed, summary.ignored, summary.measured
        ))
    }
}

pub(crate) fn parse_ignored_libtest_names(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let stripped = strip_ansi(line);
            let trimmed = stripped.trim();
            let name = trimmed.strip_suffix(": test")?;
            if name.is_empty()
                || name.contains(' ')
                || name.starts_with("test ")
                || name.starts_with("running ")
                || name.starts_with("error:")
            {
                return None;
            }
            Some(name.to_string())
        })
        .collect()
}

struct CapturedCommand {
    status: std::process::ExitStatus,
    combined: String,
}

fn run_command_capture(
    cmd: &str,
    args: &[String],
    workdir: &str,
    envs: &[(String, String)],
) -> Result<CapturedCommand> {
    let mut c = Command::new(cmd);
    c.args(args)
        .current_dir(workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in envs {
        c.env(k, v);
    }

    let mut child = c
        .spawn()
        .with_context(|| format!("failed to spawn {cmd}"))?;
    let stdout = child
        .stdout
        .take()
        .context("failed to capture child stdout")?;
    let stderr = child
        .stderr
        .take()
        .context("failed to capture child stderr")?;

    let (tx, rx) = mpsc::channel::<(bool, String)>();
    let tx_out = tx.clone();
    let stdout_handle = thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    let _ = tx_out.send((false, line));
                }
                Err(_) => break,
            }
        }
    });
    let tx_err = tx.clone();
    let stderr_handle = thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    let _ = tx_err.send((true, line));
                }
                Err(_) => break,
            }
        }
    });
    drop(tx);

    let mut stdout_buf = String::new();
    let mut stderr_buf = String::new();
    let mut last_live_printed: Option<String> = None;
    let mut append_line = |is_err: bool, line: String| {
        if is_err {
            stderr_buf.push_str(&line);
            stderr_buf.push('\n');
        } else {
            stdout_buf.push_str(&line);
            stdout_buf.push('\n');
        }
        if should_live_print_rust_test_line(&line) {
            let dedupe_key = strip_ansi(&line).trim().to_string();
            if last_live_printed.as_deref() != Some(dedupe_key.as_str()) {
                eprintln!("{line}");
                last_live_printed = Some(dedupe_key);
            }
        }
    };

    let status = loop {
        while let Ok((is_err, line)) = rx.try_recv() {
            append_line(is_err, line);
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok((is_err, line)) => append_line(is_err, line),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {}
        }
    };

    let _ = stdout_handle.join();
    let _ = stderr_handle.join();
    while let Ok((is_err, line)) = rx.try_recv() {
        append_line(is_err, line);
    }

    Ok(CapturedCommand {
        status,
        combined: format!("{}\n{}", stdout_buf, stderr_buf),
    })
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

struct ExactPocketIcWrapper {
    dir: std::path::PathBuf,
    wrapper_bin: std::path::PathBuf,
    pid_file: std::path::PathBuf,
    expected_bin: std::path::PathBuf,
    expected_ttl_secs: u64,
}

impl ExactPocketIcWrapper {
    fn new(pocketic_bin: &str, test_name: &str, ttl_secs: u64) -> Result<Self> {
        let expected_bin = fs::canonicalize(pocketic_bin)
            .with_context(|| format!("failed to canonicalize PocketIC binary {pocketic_bin}"))?;
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let unique = format!(
            "jupiter-pocketic-exact-{}-{}-{}",
            std::process::id(),
            suffix,
            test_name
                .chars()
                .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
                .collect::<String>()
        );
        let dir = env::temp_dir().join(unique);
        fs::create_dir(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
        let wrapper_bin = dir.join("pocket-ic-wrapper");
        let pid_file = dir.join("pocket-ic.pid");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$$\" > {}\nexec {} \"$@\"\n",
            shell_quote(&pid_file.display().to_string()),
            shell_quote(&expected_bin.display().to_string())
        );
        fs::write(&wrapper_bin, script)
            .with_context(|| format!("failed to write {}", wrapper_bin.display()))?;
        let mut perms = fs::metadata(&wrapper_bin)?.permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o700);
            fs::set_permissions(&wrapper_bin, perms)?;
        }

        Ok(Self {
            dir,
            wrapper_bin,
            pid_file,
            expected_bin,
            expected_ttl_secs: ttl_secs,
        })
    }

    fn bin_path(&self) -> String {
        self.wrapper_bin.display().to_string()
    }

    fn cleanup(&self) {
        let Some(pid) = self.recorded_pid() else {
            let _ = fs::remove_dir_all(&self.dir);
            return;
        };
        if self.pid_matches_expected_server(pid) {
            terminate_pid(pid);
        }
        let _ = fs::remove_dir_all(&self.dir);
    }

    fn recorded_pid(&self) -> Option<u32> {
        let raw = fs::read_to_string(&self.pid_file).ok()?;
        raw.trim().parse::<u32>().ok()
    }

    #[cfg(unix)]
    fn pid_matches_expected_server(&self, pid: u32) -> bool {
        let exe = fs::read_link(format!("/proc/{pid}/exe")).ok();
        let exe_matches = exe
            .as_ref()
            .and_then(|path| fs::canonicalize(path).ok())
            .map(|path| path == self.expected_bin)
            .unwrap_or(false);
        if !exe_matches {
            return false;
        }

        let raw = match fs::read(format!("/proc/{pid}/cmdline")) {
            Ok(raw) => raw,
            Err(_) => return false,
        };
        let parts = raw
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect::<Vec<_>>();
        parts
            .windows(2)
            .any(|pair| pair[0] == "--ttl" && pair[1] == self.expected_ttl_secs.to_string())
            && parts
                .windows(2)
                .any(|pair| pair[0] == "--port-file" && pair[1].starts_with("/tmp/pocket_ic_"))
    }

    #[cfg(not(unix))]
    fn pid_matches_expected_server(&self, _pid: u32) -> bool {
        false
    }
}

impl Drop for ExactPocketIcWrapper {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn terminate_pid(pid: u32) {
    let pid_arg = pid.to_string();
    let _ = Command::new("kill")
        .arg(&pid_arg)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let alive = Command::new("kill")
            .args(["-0", &pid_arg])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !alive {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let _ = Command::new("kill")
        .args(["-KILL", &pid_arg])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn suite_scope_label(layer: &str, component: &str) -> String {
    if component.is_empty() {
        format!("[{layer}]")
    } else {
        format!("[{layer}/{component}]")
    }
}

pub(crate) fn run_cargo_test_suite(
    outcomes: &mut Vec<ScenarioOutcome>,
    suite_label: &str,
    component: &str,
    cmd: &str,
    args: &[&str],
    workdir: &str,
    envs: &[(&str, &str)],
) -> Result<()> {
    let scope = suite_scope_label(suite_label, component);
    let full_label = format!("{scope} {} {}", cmd, args.join(" "));
    eprintln!("\n{BOLD}=== {full_label} ==={RESET}");
    let t0 = Instant::now();
    let args_owned = args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    let envs_owned = envs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect::<Vec<_>>();
    let captured = run_command_capture(cmd, &args_owned, workdir, &envs_owned)?;

    let ms = t0.elapsed().as_millis();
    if captured.status.success() {
        outcomes.push(ScenarioOutcome {
            name: format!("{scope} suite passed"),
            ms,
            passed: true,
            error: None,
        });
        eprintln!("{GREEN}✓{RESET} {scope} suite passed {DIM}({ms}ms){RESET}");
        return Ok(());
    }

    let failed_tests = parse_failed_rust_test_details(&captured.combined);
    if failed_tests.is_empty() {
        outcomes.push(ScenarioOutcome {
            name: format!("{scope} test command failed"),
            ms,
            passed: false,
            error: Some(strip_ansi(captured.combined.trim())),
        });
    } else {
        for (test_name, detail) in failed_tests {
            let short = truncate_error(&strip_ansi(&detail), 140);
            eprintln!("{RED}↳{RESET} {scope} {test_name}: {DIM}{short}{RESET}");
            outcomes.push(ScenarioOutcome {
                name: format!("{scope} {test_name}"),
                ms,
                passed: false,
                error: Some(detail),
            });
        }
    }
    eprintln!("{RED}✗{RESET} {scope} suite failed {DIM}({ms}ms){RESET}");
    Ok(())
}

fn record_exact_test_outcome(
    outcomes: &mut Vec<ScenarioOutcome>,
    scope: &str,
    test_name: &str,
    ms: u128,
    captured: &CapturedCommand,
) {
    let full_label = format!("{scope} {test_name}");
    let execution_error = exact_execution_error(
        parse_libtest_summary(&captured.combined),
        captured.status.success(),
    );

    if captured.status.success() && execution_error.is_none() {
        outcomes.push(ScenarioOutcome {
            name: full_label.clone(),
            ms,
            passed: true,
            error: None,
        });
        eprintln!("{GREEN}✓{RESET} {full_label} {DIM}({ms}ms){RESET}");
    } else {
        let status_detail = captured
            .status
            .code()
            .map(|code| format!("exit status {code}"))
            .unwrap_or_else(|| format!("{}", captured.status));
        let output = strip_ansi(captured.combined.trim());
        let detail = match (execution_error, output.is_empty()) {
            (Some(err), true) => format!("{status_detail}\n{err}"),
            (Some(err), false) => format!("{status_detail}\n{err}\n{output}"),
            (None, true) => status_detail,
            (None, false) => format!("{status_detail}\n{output}"),
        };
        outcomes.push(ScenarioOutcome {
            name: full_label.clone(),
            ms,
            passed: false,
            error: Some(detail.clone()),
        });
        let short = truncate_error(&detail, 140);
        eprintln!("{RED}✗{RESET} {full_label} {DIM}({ms}ms){RESET}");
        eprintln!("{RED}↳{RESET} {scope} {test_name}: {DIM}{short}{RESET}");
    }
}

pub(crate) struct IgnoredCargoTestSuite<'a> {
    pub(crate) suite_label: &'a str,
    pub(crate) component: &'a str,
    pub(crate) package: &'a str,
    pub(crate) test_target: &'a str,
    pub(crate) workdir: &'a str,
    pub(crate) pocketic_bin: &'a str,
    pub(crate) pocketic_idle_ttl_secs: u64,
}

pub(crate) fn run_cargo_ignored_tests_individually(
    outcomes: &mut Vec<ScenarioOutcome>,
    suite: IgnoredCargoTestSuite<'_>,
) -> Result<()> {
    let scope = suite_scope_label(suite.suite_label, suite.component);
    let list_args = vec![
        "test",
        "-p",
        suite.package,
        "--test",
        suite.test_target,
        "--",
        "--list",
        "--ignored",
    ];
    eprintln!("\n{BOLD}=== {scope} discover ignored tests ==={RESET}");
    let list_output = Command::new("cargo")
        .args(&list_args)
        .current_dir(suite.workdir)
        .env("POCKET_IC_BIN", suite.pocketic_bin)
        .env("RUST_TEST_THREADS", "1")
        .output()
        .context("failed to list ignored Disburser PocketIC tests")?;
    let list_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&list_output.stdout),
        String::from_utf8_lossy(&list_output.stderr)
    );
    if !list_output.status.success() {
        bail!("failed to list ignored Disburser PocketIC tests: {list_text}");
    }
    let tests = parse_ignored_libtest_names(&list_text);
    if tests.is_empty() {
        bail!("no ignored Disburser PocketIC tests discovered");
    }

    for test_name in tests {
        let full_label = format!("{scope} {test_name}");
        eprintln!("\n{BOLD}=== {full_label} ==={RESET}");
        let t0 = Instant::now();
        let pocketic_wrapper = ExactPocketIcWrapper::new(
            suite.pocketic_bin,
            &test_name,
            suite.pocketic_idle_ttl_secs,
        )?;
        let wrapper_bin = pocketic_wrapper.bin_path();
        let args = vec![
            "test".to_string(),
            "-p".to_string(),
            suite.package.to_string(),
            "--test".to_string(),
            suite.test_target.to_string(),
            test_name.clone(),
            "--".to_string(),
            "--exact".to_string(),
            "--ignored".to_string(),
            "--color".to_string(),
            "always".to_string(),
            "--test-threads=1".to_string(),
        ];
        let envs = vec![
            ("POCKET_IC_BIN".to_string(), wrapper_bin),
            ("POCKET_IC_MUTE_SERVER".to_string(), "1".to_string()),
            ("RUST_TEST_THREADS".to_string(), "1".to_string()),
            (
                "JUPITER_POCKETIC_IDLE_TTL_SECS".to_string(),
                suite.pocketic_idle_ttl_secs.to_string(),
            ),
        ];
        let captured = run_command_capture("cargo", &args, suite.workdir, &envs)?;
        drop(pocketic_wrapper);
        let ms = t0.elapsed().as_millis();
        record_exact_test_outcome(outcomes, &scope, &test_name, ms, &captured);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn parse_ignored_libtest_names_ignores_non_test_lines() {
        let output = "\
running 32 tests
payout_plan_uses_two_year_age_snapshot_and_clamps_at_four_years: test
not a test line
warning: something
faucet_baseline_round_accounting_without_invalid_top_up_is_stable: test

32 tests, 0 benchmarks
";
        assert_eq!(
            parse_ignored_libtest_names(output),
            vec![
                "payout_plan_uses_two_year_age_snapshot_and_clamps_at_four_years".to_string(),
                "faucet_baseline_round_accounting_without_invalid_top_up_is_stable".to_string(),
            ]
        );
    }

    #[test]
    fn parse_ignored_libtest_names_preserves_deterministic_order() {
        let output = "b_test: test\na_test: test\nc_test: test\n";
        assert_eq!(
            parse_ignored_libtest_names(output),
            vec![
                "b_test".to_string(),
                "a_test".to_string(),
                "c_test".to_string(),
            ]
        );
    }

    #[test]
    fn parse_libtest_summary_reads_exactly_one_passed() {
        let output = "\
running 1 test
test exact_name ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 19 filtered out; finished in 0.01s
";
        assert_eq!(
            parse_libtest_summary(output),
            Some(LibtestSummary {
                passed: 1,
                failed: 0,
                ignored: 0,
                measured: 0,
            })
        );
        assert_eq!(
            exact_execution_error(parse_libtest_summary(output), true),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn exact_outcome_rejects_zero_matched_tests() {
        let mut outcomes = Vec::new();
        let captured = CapturedCommand {
            status: std::process::ExitStatus::from_raw(0),
            combined: "running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 20 filtered out; finished in 0.00s\n".to_string(),
        };
        record_exact_test_outcome(
            &mut outcomes,
            "[pocketic/disburser]",
            "missing_test",
            10,
            &captured,
        );

        assert_eq!(outcomes.len(), 1);
        assert!(!outcomes[0].passed);
        assert!(outcomes[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("zero matched tests"));
    }

    #[cfg(unix)]
    #[test]
    fn exact_outcome_rejects_ignored_but_unexecuted_test() {
        let mut outcomes = Vec::new();
        let captured = CapturedCommand {
            status: std::process::ExitStatus::from_raw(0),
            combined: "running 1 test\ntest exact ... ignored\ntest result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s\n".to_string(),
        };
        record_exact_test_outcome(
            &mut outcomes,
            "[pocketic/disburser]",
            "ignored_test",
            10,
            &captured,
        );

        assert_eq!(outcomes.len(), 1);
        assert!(!outcomes[0].passed);
        assert!(outcomes[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("0 passed; 0 failed; 1 ignored; 0 measured"));
    }

    #[cfg(unix)]
    #[test]
    fn exact_outcome_rejects_one_failed_test() {
        let mut outcomes = Vec::new();
        let captured = CapturedCommand {
            status: std::process::ExitStatus::from_raw(256),
            combined: "running 1 test\ntest exact ... FAILED\nassertion failed\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n".to_string(),
        };
        record_exact_test_outcome(
            &mut outcomes,
            "[pocketic/disburser]",
            "failed_test",
            10,
            &captured,
        );

        assert_eq!(outcomes.len(), 1);
        assert!(!outcomes[0].passed);
        assert!(outcomes[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("0 passed; 1 failed; 0 ignored; 0 measured"));
    }

    #[cfg(unix)]
    #[test]
    fn command_capture_preserves_complete_tail_output() {
        let cwd = env::current_dir().expect("current dir");
        let captured = run_command_capture(
            "sh",
            &[
                "-c".to_string(),
                "printf 'stdout-head\\n'; printf 'stderr-tail\\n' >&2".to_string(),
            ],
            cwd.to_str().expect("utf8 cwd"),
            &[],
        )
        .expect("capture command should run");

        assert!(captured.status.success());
        assert!(captured.combined.contains("stdout-head"));
        assert!(captured.combined.contains("stderr-tail"));
    }

    #[cfg(unix)]
    #[test]
    fn exact_outcome_accepts_success_for_one_executed_test() {
        let mut outcomes = Vec::new();
        let captured = CapturedCommand {
            status: std::process::ExitStatus::from_raw(0),
            combined: "running 1 test\ntest exact ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n".to_string(),
        };
        record_exact_test_outcome(
            &mut outcomes,
            "[pocketic/disburser]",
            "exact",
            10,
            &captured,
        );

        assert_eq!(outcomes.len(), 1);
        assert!(outcomes[0].passed);
    }

    #[cfg(unix)]
    #[test]
    fn exact_outcome_records_one_entry_per_test_and_continues_after_failure() {
        let mut outcomes = Vec::new();
        let success = CapturedCommand {
            status: std::process::ExitStatus::from_raw(0),
            combined: "running 1 test\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n".to_string(),
        };
        let failure = CapturedCommand {
            status: std::process::ExitStatus::from_raw(256),
            combined: "running 1 test\ntest second ... FAILED\nassertion failed\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n".to_string(),
        };
        record_exact_test_outcome(&mut outcomes, "[pocketic/disburser]", "first", 10, &success);
        record_exact_test_outcome(
            &mut outcomes,
            "[pocketic/disburser]",
            "second",
            20,
            &failure,
        );
        record_exact_test_outcome(&mut outcomes, "[pocketic/disburser]", "third", 30, &success);

        assert_eq!(outcomes.len(), 3);
        assert!(outcomes[0].passed);
        assert!(!outcomes[1].passed);
        assert!(outcomes[2].passed);
        assert!(outcomes[1]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("assertion failed"));
    }

    #[cfg(unix)]
    #[test]
    fn exact_outcome_reports_abort_as_failed_test() {
        let mut outcomes = Vec::new();
        let captured = CapturedCommand {
            status: std::process::ExitStatus::from_raw(6),
            combined: String::new(),
        };
        record_exact_test_outcome(
            &mut outcomes,
            "[pocketic/disburser]",
            "aborting_test",
            10,
            &captured,
        );

        assert_eq!(outcomes.len(), 1);
        assert!(!outcomes[0].passed);
        assert!(outcomes[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("signal"));
    }
}
