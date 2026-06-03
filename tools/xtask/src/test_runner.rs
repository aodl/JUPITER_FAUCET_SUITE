use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::sync::mpsc;
use std::thread;

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
    for (is_err, line) in rx {
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
    }

    let status = child
        .wait()
        .with_context(|| format!("failed waiting for {cmd}"))?;
    let _ = stdout_handle.join();
    let _ = stderr_handle.join();

    let ms = t0.elapsed().as_millis();
    if status.success() {
        outcomes.push(ScenarioOutcome {
            name: format!("{scope} suite passed"),
            ms,
            passed: true,
            error: None,
        });
        eprintln!("{GREEN}✓{RESET} {scope} suite passed {DIM}({ms}ms){RESET}");
        return Ok(());
    }

    let combined = format!("{}\n{}", stdout_buf, stderr_buf);
    let failed_tests = parse_failed_rust_test_details(&combined);
    if failed_tests.is_empty() {
        outcomes.push(ScenarioOutcome {
            name: format!("{scope} test command failed"),
            ms,
            passed: false,
            error: Some(strip_ansi(combined.trim())),
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
