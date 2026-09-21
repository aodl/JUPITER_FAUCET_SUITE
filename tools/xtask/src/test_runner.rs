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
        if ch.is_control() && ch != '\n' && ch != '\t' {
            continue;
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
    filtered_out: usize,
}

fn parse_rust_summary(line: &str) -> Option<LibtestSummary> {
    let rest = line.strip_prefix("test result: ")?;
    let rest = rest
        .strip_prefix("ok. ")
        .or_else(|| rest.strip_prefix("FAILED. "))?;
    let parts = rest.split("; ").collect::<Vec<_>>();
    if parts.len() != 6 || !parts[5].starts_with("finished in ") || !parts[5].ends_with('s') {
        return None;
    }
    let count = |part: &str, suffix: &str| part.strip_suffix(suffix)?.parse::<usize>().ok();
    let parsed = LibtestSummary {
        passed: count(parts[0], " passed")?,
        failed: count(parts[1], " failed")?,
        ignored: count(parts[2], " ignored")?,
        measured: count(parts[3], " measured")?,
        filtered_out: count(parts[4], " filtered out")?,
    };
    let seconds = parts[5].strip_prefix("finished in ")?.strip_suffix('s')?;
    seconds
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value >= 0.0)?;
    if line.contains("test result: ok.") && parsed.failed != 0 {
        return None;
    }
    if line.contains("test result: FAILED.") && parsed.failed == 0 {
        return None;
    }
    Some(parsed)
}

fn parse_libtest_summaries(output: &str) -> Vec<LibtestSummary> {
    let mut summaries = Vec::new();
    for line in output.lines() {
        let stripped = strip_ansi(line);
        let trimmed = stripped.trim();
        if trimmed.starts_with("test result:") {
            if let Some(parsed) = parse_rust_summary(trimmed) {
                summaries.push(parsed);
            }
        }
    }
    summaries
}

fn parse_node_test_summary(output: &str) -> Option<LibtestSummary> {
    let mut tests = None;
    let mut passed = None;
    let mut failed = None;
    let mut cancelled = None;
    let mut skipped = None;
    let mut todo = None;
    for line in output.lines() {
        let stripped = strip_ansi(line);
        let words = stripped.split_whitespace().collect::<Vec<_>>();
        if words.len() != 3 || !matches!(words[0], "#" | "ℹ") {
            continue;
        }
        let Ok(count) = words[2].parse::<usize>() else {
            continue;
        };
        let slot = match words[1] {
            "tests" => &mut tests,
            "pass" => &mut passed,
            "fail" => &mut failed,
            "cancelled" => &mut cancelled,
            "skipped" => &mut skipped,
            "todo" => &mut todo,
            _ => continue,
        };
        if slot.replace(count).is_some() {
            return None;
        }
    }
    let (tests, passed, failed, cancelled, skipped, todo) =
        (tests?, passed?, failed?, cancelled?, skipped?, todo?);
    if cancelled != 0
        || tests
            != passed
                .checked_add(failed)?
                .checked_add(cancelled)?
                .checked_add(skipped)?
                .checked_add(todo)?
    {
        return None;
    }
    Some(LibtestSummary {
        passed,
        failed,
        ignored: skipped,
        measured: 0,
        filtered_out: todo,
    })
}

fn aggregate_test_summary(output: &str) -> Option<LibtestSummary> {
    let summaries = parse_libtest_summaries(output);
    let result_lines = output
        .lines()
        .filter(|line| strip_ansi(line).trim().starts_with("test result:"))
        .count();
    let running_lines = output
        .lines()
        .filter(|line| strip_ansi(line).trim().starts_with("running "))
        .count();
    let running = output
        .lines()
        .filter_map(|line| {
            let clean = strip_ansi(line);
            let words = clean.split_whitespace().collect::<Vec<_>>();
            if words.len() == 3 && words[0] == "running" && matches!(words[2], "test" | "tests") {
                words[1].parse::<usize>().ok()
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if result_lines == 0 {
        return parse_node_test_summary(output);
    }
    if parse_node_test_summary(output).is_some()
        || summaries.len() != result_lines
        || running_lines != running.len()
        || running.len() != summaries.len()
        || (!running.is_empty()
            && running.iter().zip(&summaries).any(|(count, summary)| {
                *count != summary.passed + summary.failed + summary.ignored + summary.measured
            }))
    {
        return None;
    }
    Some(
        summaries
            .into_iter()
            .fold(LibtestSummary::default(), |mut total, summary| {
                total.passed = total.passed.saturating_add(summary.passed);
                total.failed = total.failed.saturating_add(summary.failed);
                total.ignored = total.ignored.saturating_add(summary.ignored);
                total.measured = total.measured.saturating_add(summary.measured);
                total.filtered_out = total.filtered_out.saturating_add(summary.filtered_out);
                total
            }),
    )
}

fn cargo_running_target_name(line: &str) -> Option<String> {
    let line = strip_ansi(line);
    let line = line.trim().strip_prefix("Running ")?;
    let binary = line.rsplit_once(" (")?.1.strip_suffix(')')?;
    let stem = std::path::Path::new(binary).file_name()?.to_str()?;
    let (name, hash) = stem.rsplit_once('-')?;
    (!name.is_empty() && !hash.is_empty() && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| name.to_string())
}

fn cargo_target_execution_error(output: &str, args: &[&str], workdir: &str) -> Option<String> {
    let headers = output
        .lines()
        .filter_map(|line| {
            let clean = strip_ansi(line);
            let trimmed = clean.trim();
            trimmed
                .starts_with("Running ")
                .then(|| (trimmed.to_string(), cargo_running_target_name(trimmed)))
        })
        .collect::<Vec<_>>();
    let summaries = output
        .lines()
        .filter(|line| strip_ansi(line).trim().starts_with("test result:"))
        .count();
    if headers.len() != summaries || headers.iter().any(|(_, name)| name.is_none()) {
        return Some(format!(
            "Cargo reported {} targets but {} terminal summaries",
            headers.len(),
            summaries
        ));
    }
    let expected = if args.contains(&"--workspace") {
        let metadata = match Command::new("cargo")
            .args(["metadata", "--no-deps", "--format-version=1", "--locked"])
            .current_dir(workdir)
            .output()
        {
            Ok(metadata) => metadata,
            Err(error) => return Some(format!("Cargo metadata target discovery failed: {error}")),
        };
        if !metadata.status.success() {
            return Some("Cargo metadata target discovery failed".to_string());
        }
        let value: serde_json::Value = match serde_json::from_slice(&metadata.stdout) {
            Ok(value) => value,
            Err(error) => {
                return Some(format!(
                    "Cargo metadata target discovery was malformed: {error}"
                ))
            }
        };
        let Some(packages) = value["packages"].as_array() else {
            return Some("Cargo metadata target discovery omitted packages".to_string());
        };
        packages
            .iter()
            .flat_map(|package| package["targets"].as_array().into_iter().flatten())
            .filter(|target| {
                target["kind"].as_array().is_some_and(|kinds| {
                    kinds
                        .iter()
                        .any(|kind| kind == "lib" || kind == "rlib" || kind == "cdylib")
                })
            })
            .filter_map(|target| target["name"].as_str().map(str::to_string))
            .collect::<Vec<_>>()
    } else if let Some(index) = args.iter().position(|arg| *arg == "--test") {
        let Some(name) = args.get(index + 1) else {
            return Some("Cargo --test target was omitted".to_string());
        };
        vec![name.to_string()]
    } else if let Some(index) = args.iter().position(|arg| *arg == "-p") {
        let Some(name) = args.get(index + 1) else {
            return Some("Cargo -p package was omitted".to_string());
        };
        vec![name.replace('-', "_")]
    } else {
        Vec::new()
    };
    if headers.len() != expected.len()
        || expected.iter().any(|target| {
            headers
                .iter()
                .filter(|(_, name)| name.as_deref() == Some(target))
                .count()
                != 1
        })
    {
        return Some(format!(
            "Cargo target execution mismatch: expected {expected:?}, observed {headers:?}"
        ));
    }
    None
}

fn cargo_target_selections(output: &str, listing: bool) -> Option<BTreeMap<String, usize>> {
    let mut targets = BTreeMap::new();
    let mut current = None;
    for line in output.lines() {
        let clean = strip_ansi(line);
        let line = clean.trim();
        if line.starts_with("Running ") {
            if current.is_some() {
                return None;
            }
            current = Some(cargo_running_target_name(line)?);
        } else if current.is_some() {
            let count = if listing {
                line.split_once(' ').and_then(|(count, rest)| {
                    (rest.starts_with("test, ") || rest.starts_with("tests, "))
                        .then(|| count.parse::<usize>().ok())
                        .flatten()
                })
            } else {
                line.strip_prefix("running ")
                    .and_then(|rest| rest.split_once(' '))
                    .and_then(|(count, word)| {
                        matches!(word, "test" | "tests")
                            .then(|| count.parse::<usize>().ok())
                            .flatten()
                    })
            };
            if let Some(count) = count {
                let name = current.take()?;
                if targets.insert(name, count).is_some() {
                    return None;
                }
            }
        }
    }
    if current.is_some() || targets.is_empty() {
        None
    } else {
        Some(targets)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CargoTargetResult {
    selected: usize,
    summary: LibtestSummary,
}

fn cargo_target_results(output: &str) -> Option<BTreeMap<String, CargoTargetResult>> {
    let mut results = BTreeMap::new();
    let mut current: Option<(String, Option<usize>)> = None;
    for raw in output.lines() {
        let clean = strip_ansi(raw);
        let line = clean.trim();
        if line.starts_with("Running ") {
            if current.is_some() {
                return None;
            }
            current = Some((cargo_running_target_name(line)?, None));
        } else if let Some(rest) = line.strip_prefix("running ") {
            let (count, word) = rest.split_once(' ')?;
            if !matches!(word, "test" | "tests") {
                return None;
            }
            let (_, selected) = current.as_mut()?;
            if selected.replace(count.parse().ok()?).is_some() {
                return None;
            }
        } else if line.starts_with("test result:") {
            let (name, selected) = current.take()?;
            let selected = selected?;
            let summary = parse_rust_summary(line)?;
            if selected != summary.passed + summary.failed + summary.ignored + summary.measured
                || results
                    .insert(name, CargoTargetResult { selected, summary })
                    .is_some()
            {
                return None;
            }
        }
    }
    (current.is_none() && !results.is_empty()).then_some(results)
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

fn suite_execution_error(summary: Option<LibtestSummary>, status_success: bool) -> Option<String> {
    if !status_success {
        return None;
    }
    let Some(summary) = summary else {
        return Some(
            "test command exited successfully without a complete test summary".to_string(),
        );
    };
    if summary.failed != 0 {
        return Some(format!(
            "test command exited successfully but reported {} failed tests",
            summary.failed
        ));
    }
    if summary.passed == 0 {
        return Some(format!(
            "test command exited successfully but executed no passing tests ({} ignored, {} filtered out)",
            summary.ignored, summary.filtered_out
        ));
    }
    None
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
    capture_error: Option<String>,
}

#[cfg(unix)]
fn terminate_owned_process_group(pid: u32) {
    // Every captured command starts a new process group. A negative PID is
    // therefore scoped to this command and cannot target our terminal group.
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    // `kill` returning only proves signal delivery. Wait briefly for members
    // to leave the live process set before reporting completed cleanup.
    let expected_group = pid.to_string();
    for _ in 0..30 {
        let live = Command::new("ps")
            .args(["-e", "-o", "pgid=,stat="])
            .output()
            .ok()
            .is_some_and(|output| {
                String::from_utf8_lossy(&output.stdout).lines().any(|line| {
                    let mut fields = line.split_whitespace();
                    fields.next() == Some(expected_group.as_str())
                        && fields.next().is_some_and(|state| !state.starts_with('Z'))
                })
            });
        if !live {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn run_command_capture(
    cmd: &str,
    args: &[String],
    workdir: &str,
    envs: &[(String, String)],
) -> Result<CapturedCommand> {
    let docker_cidfile = (cmd == "npm" && args.iter().any(|arg| arg == "test:frontend-browser"))
        .then(|| {
            env::temp_dir().join(format!(
                "jupiter-browser-{}-{}.cid",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ))
        });
    let mut c = Command::new(cmd);
    c.args(args)
        .current_dir(workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    for (k, v) in envs {
        c.env(k, v);
    }
    if let Some(path) = &docker_cidfile {
        c.env("JUPITER_DOCKER_CIDFILE", path);
    }

    let timeout_value = envs
        .iter()
        .find(|(key, _)| key == "JUPITER_TEST_COMMAND_TIMEOUT_SECS")
        .map(|(_, value)| value.clone())
        .or_else(|| env::var("JUPITER_TEST_COMMAND_TIMEOUT_SECS").ok());
    let timeout = match timeout_value {
        Some(value) => {
            let seconds = value
                .parse::<u64>()
                .context("invalid JUPITER_TEST_COMMAND_TIMEOUT_SECS")?;
            anyhow::ensure!(
                seconds > 0,
                "JUPITER_TEST_COMMAND_TIMEOUT_SECS must be positive"
            );
            Duration::from_secs(seconds)
        }
        None => Duration::from_secs(3 * 60 * 60),
    };
    let deadline = Instant::now()
        .checked_add(timeout)
        .context("JUPITER_TEST_COMMAND_TIMEOUT_SECS is too large")?;
    // Cargo emits target headers on stderr and libtest emits summaries on
    // stdout. Two independent readers cannot recover their write order.
    // Give both child descriptors the same kernel stream before spawning.
    #[cfg(unix)]
    let (reader, writer) = std::os::unix::net::UnixStream::pair()?;
    #[cfg(unix)]
    {
        use std::os::fd::OwnedFd;
        c.stdout(Stdio::from(OwnedFd::from(writer.try_clone()?)))
            .stderr(Stdio::from(OwnedFd::from(writer)));
    }
    let mut child = c
        .spawn()
        .with_context(|| format!("failed to spawn {cmd}"))?;
    // Command retains its configured descriptors after spawn; close the
    // parent's copies so EOF reflects only the owned child/process group.
    drop(c);
    let owned_pid = child.id();
    #[cfg(unix)]
    let output = reader;
    #[cfg(not(unix))]
    let output = child
        .stdout
        .take()
        .context("failed to capture child stdout")?;

    let (tx, rx) = mpsc::channel::<String>();
    #[cfg(test)]
    let reader_delay = envs
        .iter()
        .find(|(key, _)| key == "JUPITER_TEST_STDERR_READER_DELAY_MS")
        .and_then(|(_, value)| value.parse::<u64>().ok())
        .unwrap_or(0);
    let output_handle = thread::spawn(move || {
        #[cfg(test)]
        thread::sleep(Duration::from_millis(reader_delay));
        let reader = BufReader::new(output);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    let _ = tx.send(line);
                }
                Err(_) => break,
            }
        }
    });

    let mut combined_buf = String::new();
    let mut last_live_printed: Option<String> = None;
    let mut append_line = |line: String| {
        combined_buf.push_str(&line);
        combined_buf.push('\n');
        if should_live_print_rust_test_line(&line) {
            let dedupe_key = strip_ansi(&line).trim().to_string();
            if last_live_printed.as_deref() != Some(dedupe_key.as_str()) {
                eprintln!("{line}");
                last_live_printed = Some(dedupe_key);
            }
        }
    };

    let mut status = None;
    let mut capture_error = None;
    let mut exited_at = None;
    loop {
        while let Ok(line) = rx.try_recv() {
            append_line(line);
        }
        if status.is_none() {
            status = child.try_wait()?;
            if status.is_some() {
                exited_at = Some(Instant::now());
            }
        }
        if status.is_some() {
            match rx.try_recv() {
                Ok(line) => append_line(line),
                Err(mpsc::TryRecvError::Disconnected) => break,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if Instant::now() >= deadline {
            capture_error = Some(
                "test command exceeded its configured timeout, including output collection"
                    .to_string(),
            );
        } else if exited_at.is_some_and(|at| at.elapsed() >= Duration::from_millis(200)) {
            capture_error =
                Some("test command exited while a descendant kept output pipes open".to_string());
        }
        if let Some(reason) = &capture_error {
            append_line(reason.clone());
            #[cfg(unix)]
            terminate_owned_process_group(owned_pid);
            if status.is_none() {
                let _ = child.kill();
                status = Some(child.wait().context("failed to reap test command")?);
            }
            break;
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => append_line(line),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {}
        }
    }

    // Group termination closes owned descendant pipes. Never let joins extend
    // past the command deadline if an escaped process still owns a descriptor.
    if capture_error.is_some() {
        let drain_deadline = Instant::now() + Duration::from_millis(300);
        while !output_handle.is_finished() && Instant::now() < drain_deadline {
            while let Ok(line) = rx.try_recv() {
                append_line(line);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    if capture_error.is_none() || output_handle.is_finished() {
        let _ = output_handle.join();
    }
    while let Ok(line) = rx.try_recv() {
        append_line(line);
    }
    if let Some(path) = docker_cidfile {
        if capture_error.is_some() {
            if let Ok(id) = fs::read_to_string(&path) {
                let id = id.trim();
                if id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    let _ = Command::new("timeout")
                        .args(["5s", "docker", "rm", "-f", id])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                }
            }
        }
        let _ = fs::remove_file(path);
    }

    Ok(CapturedCommand {
        status: status.context("test command ended without an exit status")?,
        combined: combined_buf,
        capture_error,
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
    verification: (&[(&str, &str)], bool),
) -> Result<()> {
    let (envs, require_test_execution) = verification;
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
    let execution_error = if require_test_execution && captured.status.success() {
        let summary = if cmd == "npm" {
            if captured
                .combined
                .lines()
                .any(|line| strip_ansi(line).trim().starts_with("test result:"))
            {
                None
            } else {
                parse_node_test_summary(&captured.combined)
            }
        } else {
            aggregate_test_summary(&captured.combined)
        };
        if let Some(summary) = summary {
            if cmd == "npm" {
                eprintln!("[test-execution] {scope}: passed={}, failed={}, skipped={}, todo={}, cancelled=0",
                    summary.passed, summary.failed, summary.ignored, summary.filtered_out);
            } else {
                eprintln!("[test-execution] {scope}: passed={}, failed={}, ignored={}, measured={}, filtered={}",
                    summary.passed, summary.failed, summary.ignored, summary.measured, summary.filtered_out);
            }
        }
        let basic_error = suite_execution_error(summary, captured.status.success()).or_else(|| {
            (cmd == "cargo")
                .then(|| cargo_target_execution_error(&captured.combined, args, workdir))
                .flatten()
        });
        if basic_error.is_some() || cmd != "cargo" {
            basic_error
        } else {
            let mut list_args = args_owned.clone();
            list_args.push("--list".to_string());
            let listed = run_command_capture(cmd, &list_args, workdir, &envs_owned)?;
            if !listed.status.success() || listed.capture_error.is_some() {
                Some(format!(
                    "Cargo target selection discovery failed: {} {}",
                    listed.status,
                    listed.capture_error.unwrap_or_default()
                ))
            } else {
                match (cargo_target_selections(&listed.combined, true),
                       cargo_target_results(&captured.combined)) {
                    (Some(discovered), Some(results)) if discovered.len() == results.len()
                        && discovered.iter().all(|(target, count)| results.get(target).is_some_and(|result| result.selected == *count)) => {
                        let mut skipped_target = None;
                        for (target, count) in discovered {
                            let summary = results[&target].summary;
                            let executed = summary.passed + summary.failed + summary.measured;
                            eprintln!("[target-selection] {target}: discovered={count}, selected={}, passed={}, failed={}, ignored={}, measured={}, filtered={}, executed={executed}",
                                results[&target].selected, summary.passed, summary.failed, summary.ignored, summary.measured, summary.filtered_out);
                            // Empty library targets are legitimate. A selected behavioural
                            // target with only ignored bodies needs an explicit suite exclusion,
                            // not credit from another target's passing tests.
                            if count > 0 && executed == 0 {
                                skipped_target = Some(format!("Cargo target {target} selected {count} tests but executed no test bodies ({} ignored)", summary.ignored));
                            }
                        }
                        skipped_target
                    }
                    (discovered, results) => Some(format!(
                        "Cargo target selection/execution mismatch: discovered {discovered:?}, results {results:?}")),
                }
            }
        }
    } else {
        None
    };
    if captured.status.success() && captured.capture_error.is_none() && execution_error.is_none() {
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
        let status_detail = format!("{}", captured.status);
        let diagnostic = strip_ansi(captured.combined.trim());
        let detail = captured
            .capture_error
            .clone()
            .or(execution_error)
            .map(|reason| format!("{status_detail}: {reason}\n{diagnostic}"))
            .unwrap_or_else(|| format!("{status_detail}\n{diagnostic}"));
        outcomes.push(ScenarioOutcome {
            name: format!("{scope} test command failed"),
            ms,
            passed: false,
            error: Some(detail),
        });
    } else {
        for (test_name, detail) in failed_tests {
            let detail = if let Some(reason) = &captured.capture_error {
                format!("{}: {reason}\n{detail}", captured.status)
            } else {
                detail
            };
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
        aggregate_test_summary(&captured.combined),
        captured.status.success(),
    );

    if captured.status.success() && captured.capture_error.is_none() && execution_error.is_none() {
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
        let detail = match (
            captured.capture_error.clone().or(execution_error),
            output.is_empty(),
        ) {
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
    let list_args = [
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
    let list_captured = run_command_capture(
        "cargo",
        &list_args
            .iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>(),
        suite.workdir,
        &[
            ("POCKET_IC_BIN".to_string(), suite.pocketic_bin.to_string()),
            ("RUST_TEST_THREADS".to_string(), "1".to_string()),
        ],
    )
    .context("failed to list ignored Disburser PocketIC tests")?;
    if !list_captured.status.success() || list_captured.capture_error.is_some() {
        bail!(
            "failed to list ignored Disburser PocketIC tests: {}\n{}",
            list_captured.status,
            list_captured.combined
        );
    }
    let tests = parse_ignored_libtest_names(&list_captured.combined);
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
    fn tiny_workspace(alpha: &str, beta: Option<&str>) -> std::path::PathBuf {
        let root = env::temp_dir().join(format!(
            "jupiter-runner-targets-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let members = if beta.is_some() {
            "[\"alpha\", \"beta\"]"
        } else {
            "[\"alpha\"]"
        };
        fs::write(
            root.join("Cargo.toml"),
            format!("[workspace]\nresolver = \"2\"\nmembers = {members}\n"),
        )
        .unwrap();
        for (name, source) in [("alpha", Some(alpha)), ("beta", beta)] {
            if let Some(source) = source {
                let dir = root.join(name).join("src");
                fs::create_dir_all(&dir).unwrap();
                fs::write(
                    root.join(name).join("Cargo.toml"),
                    format!(
                        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"
                    ),
                )
                .unwrap();
                fs::write(dir.join("lib.rs"), source).unwrap();
            }
        }
        root
    }

    #[cfg(unix)]
    #[test]
    fn cargo_wrapper_accepts_delayed_header_reader_for_one_and_multiple_targets() {
        for beta in [None, Some("#[test] fn beta_passes() {}\n")] {
            let root = tiny_workspace("#[test] fn alpha_passes() {}\n", beta);
            let mut outcomes = Vec::new();
            run_cargo_test_suite(
                &mut outcomes,
                "unit",
                "ordered-probe",
                "cargo",
                &[
                    "test",
                    "--workspace",
                    "--lib",
                    "--offline",
                    "--",
                    "--color",
                    "never",
                ],
                root.to_str().unwrap(),
                (&[("JUPITER_TEST_STDERR_READER_DELAY_MS", "80")], true),
            )
            .unwrap();
            assert!(
                outcomes.iter().all(|outcome| outcome.passed),
                "{outcomes:?}"
            );
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn cargo_wrapper_rejects_an_entirely_ignored_behavioural_target() {
        let root = tiny_workspace("#[test] fn alpha_passes() {}\n", Some("#[test] #[ignore] fn beta_never_runs() {}\n#[test] #[ignore] fn beta_also_never_runs() {}\n"));
        let mut outcomes = Vec::new();
        run_cargo_test_suite(
            &mut outcomes,
            "unit",
            "ignored-probe",
            "cargo",
            &[
                "test",
                "--workspace",
                "--lib",
                "--offline",
                "--",
                "--color",
                "never",
            ],
            root.to_str().unwrap(),
            (&[], true),
        )
        .unwrap();
        assert!(
            outcomes.iter().any(|outcome| !outcome.passed),
            "ignored beta must not be reported as executed: {outcomes:?}"
        );
        fs::remove_dir_all(root).unwrap();
    }

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
            aggregate_test_summary(output),
            Some(LibtestSummary {
                passed: 1,
                failed: 0,
                ignored: 0,
                measured: 0,
                filtered_out: 19,
            })
        );
        assert_eq!(
            exact_execution_error(aggregate_test_summary(output), true),
            None
        );
    }

    #[test]
    fn suite_summary_aggregates_multiple_ansi_libtest_targets() {
        let output = "running 3 tests\n\u{1b}[32mtest result:\u{1b}[0m ok. 2 passed; 0 failed; 1 ignored; 0 measured; 3 filtered out; finished in 0.01s\n\
running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
        assert_eq!(
            aggregate_test_summary(output),
            Some(LibtestSummary {
                passed: 2,
                failed: 0,
                ignored: 1,
                measured: 0,
                filtered_out: 3,
            })
        );
        assert_eq!(
            suite_execution_error(aggregate_test_summary(output), true),
            None
        );
    }

    #[test]
    fn rust_summary_accepts_cargo_color_reset_control_byte() {
        let output = "running 1 test\ntest result: \u{1b}[32mok\u{1b}[m\u{f}. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";
        assert_eq!(aggregate_test_summary(output).unwrap().passed, 1);
    }

    #[test]
    fn suite_summary_accepts_complete_node_test_output() {
        let output = "# tests 7\n# pass 6\n# fail 0\n# cancelled 0\n# skipped 1\n# todo 0\n";
        assert_eq!(
            aggregate_test_summary(output),
            Some(LibtestSummary {
                passed: 6,
                failed: 0,
                ignored: 1,
                measured: 0,
                filtered_out: 0,
            })
        );
    }

    #[test]
    fn suite_summary_accepts_current_node_info_marker() {
        let output =
            "ℹ tests 5\nℹ suites 0\nℹ pass 5\nℹ fail 0\nℹ cancelled 0\nℹ skipped 0\nℹ todo 0\n";
        assert_eq!(
            aggregate_test_summary(output),
            Some(LibtestSummary {
                passed: 5,
                failed: 0,
                ignored: 0,
                measured: 0,
                filtered_out: 0,
            })
        );
    }

    #[test]
    fn suite_summary_rejects_missing_malformed_and_zero_execution() {
        assert!(suite_execution_error(None, true).is_some());
        assert!(suite_execution_error(parse_node_test_summary("# tests seven\n"), true).is_some());
        let zero = aggregate_test_summary(
            "running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.00s\n",
        );
        assert!(suite_execution_error(zero, true)
            .unwrap()
            .contains("executed no passing tests"));
    }

    #[test]
    fn suite_summary_rejects_incomplete_conflicting_and_unfinished_targets() {
        for output in [
            "test result: ok. 1 passed\n",
            "test result: FAILED. 1 passed\n",
            "running many tests\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
            "running 1 test\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\nRunning unittests B\nrunning 42 tests\n",
        ] {
            assert!(suite_execution_error(aggregate_test_summary(output), true).is_some(), "accepted {output:?}");
        }
    }

    #[test]
    fn cargo_target_reconciliation_rejects_duplicate_or_missing_execution() {
        let complete = "Running unittests src/lib.rs (target/debug/deps/jupiter_faucet-123)\nrunning 1 test\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";
        let duplicated = format!("{complete}{complete}");
        assert!(cargo_target_execution_error(
            &duplicated,
            &["test", "-p", "jupiter-faucet", "--lib"],
            "."
        )
        .is_some());
        assert!(cargo_target_execution_error("running 1 test\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n", &["test", "-p", "jupiter-faucet", "--lib"], ".").is_some());
        assert!(cargo_target_execution_error(
            complete,
            &["test", "-p", "jupiter-faucet", "--lib"],
            "."
        )
        .is_none());
    }

    #[test]
    fn cargo_target_selection_distinguishes_empty_and_missing_targets() {
        let header_a = "Running unittests src/lib.rs (target/debug/deps/alpha-123)\n";
        let header_b = "Running unittests src/lib.rs (target/debug/deps/beta-456)\n";
        let listed = format!(
            "{header_a}one: test\n\n1 test, 0 benchmarks\n{header_b}\n0 tests, 0 benchmarks\n"
        );
        let executed = format!("{header_a}running 1 test\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n{header_b}running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n");
        assert_eq!(
            cargo_target_selections(&listed, true),
            cargo_target_selections(&executed, false)
        );
        assert_ne!(
            cargo_target_selections(&listed, true),
            cargo_target_selections(&format!("{header_a}running 1 test\n"), false)
        );
    }

    #[test]
    fn cargo_target_reconciliation_accepts_real_external_integration_path() {
        let header = "Running ../../tests/pocketic/jupiter_faucet_integration.rs (target/debug/deps/jupiter_faucet_integration-123)\n";
        let listed = format!("{header}representative: test\n\n1 test, 0 benchmarks\n");
        let executed = format!("{header}running 1 test\ntest representative ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n");
        assert!(cargo_target_execution_error(
            &executed,
            &[
                "test",
                "-p",
                "jupiter-faucet",
                "--test",
                "jupiter_faucet_integration"
            ],
            "."
        )
        .is_none());
        assert_eq!(
            cargo_target_selections(&listed, true),
            cargo_target_selections(&executed, false)
        );
        assert_eq!(
            cargo_target_selections(&executed, false),
            Some(BTreeMap::from([(
                "jupiter_faucet_integration".to_string(),
                1
            )]))
        );
    }

    #[test]
    fn node_summary_rejects_inconsistent_and_cancelled_execution() {
        for output in [
            "# tests 0\n# pass 1\n# fail 0\n# cancelled 0\n# skipped 0\n# todo 0\n",
            "# tests 2\n# pass 1\n# fail 0\n# cancelled 1\n# skipped 0\n# todo 0\n",
            "# tests 1\n# pass 1\n# fail 0\n# cancelled 0\n# skipped 0\n# todo 0\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
        ] {
            assert!(suite_execution_error(aggregate_test_summary(output), true).is_some(), "accepted {output:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn actual_libtest_wrapper_rejects_an_unmatched_filter() {
        let temp = std::env::temp_dir().join(format!(
            "jupiter-runner-negative-control-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        let source = temp.join("probe.rs");
        let binary = temp.join("probe-test");
        std::fs::write(&source, "#[test] fn intended_probe() {}\n").unwrap();
        let compile = Command::new("rustc")
            .args([
                "--test",
                source.to_str().unwrap(),
                "-o",
                binary.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(compile.status.success());
        let mut outcomes = Vec::new();
        run_cargo_test_suite(
            &mut outcomes,
            "unit",
            "probe",
            binary.to_str().unwrap(),
            &["misspelled_probe"],
            temp.to_str().unwrap(),
            (&[], true),
        )
        .unwrap();
        assert_eq!(outcomes.len(), 1);
        assert!(
            !outcomes[0].passed,
            "the suite wrapper must reject zero execution"
        );
        assert!(outcomes[0]
            .error
            .as_deref()
            .unwrap()
            .contains("executed no passing tests"));
        std::fs::remove_file(source).unwrap();
        std::fs::remove_file(binary).unwrap();
        std::fs::remove_dir(temp).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn exact_outcome_rejects_zero_matched_tests() {
        let mut outcomes = Vec::new();
        let captured = CapturedCommand {
            status: std::process::ExitStatus::from_raw(0),
            capture_error: None,
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
            capture_error: None,
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
            capture_error: None,
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
    fn command_capture_terminates_and_marks_a_timeout() {
        let cwd = env::current_dir().expect("current dir");
        let captured = run_command_capture(
            "sh",
            &["-c".to_string(), "sleep 5".to_string()],
            cwd.to_str().expect("utf8 cwd"),
            &[(
                "JUPITER_TEST_COMMAND_TIMEOUT_SECS".to_string(),
                "1".to_string(),
            )],
        )
        .expect("capture command should time out cleanly");
        assert!(!captured.status.success());
        assert!(captured.combined.contains("configured timeout"));
    }

    #[cfg(unix)]
    fn assert_descendant_gone(output: &str) {
        let pid = output
            .lines()
            .find_map(|line| {
                line.strip_prefix("descendant=")
                    .and_then(|value| value.parse::<u32>().ok())
            })
            .expect("descendant PID diagnostic must survive");
        let state = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok();
        assert!(
            state.is_none()
                || state
                    .as_deref()
                    .is_some_and(|text| text.split_whitespace().nth(2) == Some("Z")),
            "owned descendant {pid} remained live: {state:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn timeout_terminates_owned_descendant_and_bounds_pipe_collection() {
        let cwd = env::current_dir().unwrap();
        let start = Instant::now();
        let captured = run_command_capture(
            "sh",
            &[
                "-c".to_string(),
                "sleep 8 & echo descendant=$!; echo diagnostic-before-timeout; wait".to_string(),
            ],
            cwd.to_str().unwrap(),
            &[(
                "JUPITER_TEST_COMMAND_TIMEOUT_SECS".to_string(),
                "1".to_string(),
            )],
        )
        .unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        assert!(captured
            .capture_error
            .as_deref()
            .unwrap()
            .contains("timeout"));
        assert!(captured.combined.contains("diagnostic-before-timeout"));
        assert_descendant_gone(&captured.combined);
        let mut outcomes = Vec::new();
        let scope = suite_scope_label("unit", "timeout-probe");
        record_exact_test_outcome(
            &mut outcomes,
            &scope,
            "owned descendant",
            start.elapsed().as_millis(),
            &captured,
        );
        assert!(!outcomes[0].passed);
        assert!(outcomes[0]
            .error
            .as_deref()
            .unwrap()
            .contains("diagnostic-before-timeout"));
    }

    #[cfg(unix)]
    #[test]
    fn timeout_preserves_unterminated_diagnostic_line() {
        let cwd = env::current_dir().unwrap();
        let captured = run_command_capture(
            "sh",
            &[
                "-c".to_string(),
                "printf partial-diagnostic; sleep 8".to_string(),
            ],
            cwd.to_str().unwrap(),
            &[(
                "JUPITER_TEST_COMMAND_TIMEOUT_SECS".to_string(),
                "1".to_string(),
            )],
        )
        .unwrap();
        assert!(captured
            .capture_error
            .as_deref()
            .unwrap()
            .contains("timeout"));
        assert!(captured.combined.contains("partial-diagnostic"));
    }

    #[cfg(unix)]
    #[test]
    fn suite_wrapper_records_timeout_and_retains_diagnostic() {
        let cwd = env::current_dir().unwrap();
        let mut outcomes = Vec::new();
        let start = Instant::now();
        run_cargo_test_suite(
            &mut outcomes,
            "unit",
            "timeout-probe",
            "sh",
            &[
                "-c",
                "sleep 8 & echo descendant=$!; echo diagnostic-before-timeout; wait",
            ],
            cwd.to_str().unwrap(),
            (&[("JUPITER_TEST_COMMAND_TIMEOUT_SECS", "1")], false),
        )
        .unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        assert_eq!(outcomes.len(), 1);
        assert!(!outcomes[0].passed);
        let detail = outcomes[0].error.as_deref().unwrap();
        assert!(detail.contains("configured timeout"));
        assert!(detail.contains("diagnostic-before-timeout"));
        assert_descendant_gone(detail);
    }

    #[cfg(unix)]
    #[test]
    fn exited_parent_with_inherited_pipes_is_bounded_and_failed() {
        let cwd = env::current_dir().unwrap();
        let start = Instant::now();
        let captured = run_command_capture(
            "sh",
            &[
                "-c".to_string(),
                "sleep 8 & echo descendant=$!; echo parent-finished; exit 0".to_string(),
            ],
            cwd.to_str().unwrap(),
            &[(
                "JUPITER_TEST_COMMAND_TIMEOUT_SECS".to_string(),
                "1".to_string(),
            )],
        )
        .unwrap();
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(captured
            .capture_error
            .as_deref()
            .unwrap()
            .contains("descendant"));
        assert!(captured.combined.contains("parent-finished"));
        assert_descendant_gone(&captured.combined);
    }

    #[test]
    fn invalid_timeout_is_rejected_before_spawning() {
        let cwd = env::current_dir().unwrap();
        let marker =
            env::temp_dir().join(format!("jupiter-invalid-timeout-{}", std::process::id()));
        for value in ["0", "not-a-number", "18446744073709551615"] {
            let result = run_command_capture(
                "sh",
                &[
                    "-c".to_string(),
                    format!("printf started > '{}'", marker.display()),
                ],
                cwd.to_str().unwrap(),
                &[(
                    "JUPITER_TEST_COMMAND_TIMEOUT_SECS".to_string(),
                    value.to_string(),
                )],
            );
            assert!(result.is_err());
            assert!(
                !marker.exists(),
                "invalid timeout {value} started the child"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn exact_outcome_accepts_success_for_one_executed_test() {
        let mut outcomes = Vec::new();
        let captured = CapturedCommand {
            status: std::process::ExitStatus::from_raw(0),
            capture_error: None,
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
            capture_error: None,
            combined: "running 1 test\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n".to_string(),
        };
        let failure = CapturedCommand {
            status: std::process::ExitStatus::from_raw(256),
            capture_error: None,
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
            capture_error: None,
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
