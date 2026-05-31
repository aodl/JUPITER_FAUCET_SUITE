use crate::constants::{DIM, LOCAL_ENVIRONMENT, LOCAL_IDENTITY, POCKET_IC_SERVER_VERSION, RESET};
use crate::workspace::repo_root;
use anyhow::{bail, Context, Result};
use candid::Principal;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn is_suppressed_icp_success_stderr_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.is_empty()
        || trimmed.contains("] Cycles: ")
        || (trimmed.contains(" UTC: [Canister ") && trimmed.contains("] "))
        || trimmed.contains(" canister created with canister id:")
        || trimmed.starts_with("Installed code for canister ")
        || trimmed.starts_with("Reinstalled code for canister ")
}

pub(crate) fn run_icp(args: &[&str]) -> Result<String> {
    let root = repo_root();
    let mut cmd = Command::new("icp");

    cmd.args(["--project-root-override", &root]);
    cmd.args(args);

    let rendered_cmd = format!(
        "icp {}",
        cmd.get_args()
            .map(|s| s.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    );
    let verbose = env::var("VERBOSE_ICP")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if verbose {
        eprintln!("▶ {rendered_cmd}");
    }

    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("failed to spawn icp")?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() {
        for line in stderr.lines() {
            if is_suppressed_icp_success_stderr_line(line) {
                continue;
            }
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                eprintln!("{trimmed}");
            }
        }
    } else if !stderr.trim().is_empty() {
        eprint!("{stderr}");
    }

    if !output.status.success() {
        eprintln!("▶ {rendered_cmd}");
        bail!("icp {:?} failed", args);
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(crate) fn run_icp_with_identity(args: &[&str]) -> Result<String> {
    let mut owned = args.iter().map(|arg| (*arg).to_string()).collect::<Vec<_>>();
    owned.push("--identity".to_string());
    owned.push(LOCAL_IDENTITY.to_string());
    let refs = owned.iter().map(|arg| arg.as_str()).collect::<Vec<_>>();
    run_icp(&refs)
}

pub(crate) fn stop_local_network_best_effort(project_root: &str) -> Result<()> {
    let output = Command::new("icp")
        .args([
            "--project-root-override",
            project_root,
            "network",
            "stop",
            LOCAL_ENVIRONMENT,
        ])
        .output()
        .context("failed to run icp network stop local")?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let text = format!("{stdout}\n{stderr}");

    if !output.status.success() && !text.contains("network 'local' is not running") {
        bail!("failed to stop local network: {text}");
    }

    Ok(())
}

pub(crate) fn local_replica_host() -> String {
    if let Ok(host) = env::var("ICP_LOCAL_HOST") {
        let trimmed = host.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    "http://localhost:4943".to_string()
}

pub(crate) fn principal_of_identity() -> Result<Principal> {
    let p = run_icp(&["identity", "principal", "--identity", LOCAL_IDENTITY])?;
    Ok(Principal::from_text(p.trim())?)
}

fn validate_pocketic_binary(path: &Path) -> Result<()> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to inspect PocketIC binary at {}", path.display()))?;
    if !metadata.is_file() || metadata.len() == 0 {
        bail!("PocketIC binary at {} is missing or empty", path.display());
    }
    let output = Command::new(path)
        .arg("--version")
        .output()
        .with_context(|| format!("failed to run {} --version", path.display()))?;
    if !output.status.success() {
        bail!("{} --version failed", path.display());
    }
    let version = String::from_utf8_lossy(&output.stdout);
    let expected = format!("pocket-ic-server {POCKET_IC_SERVER_VERSION}");
    if version.trim() != expected {
        bail!(
            "PocketIC binary at {} reports `{}`; expected `{expected}`",
            path.display(),
            version.trim()
        );
    }
    Ok(())
}

fn discover_pocketic_binary() -> Option<PathBuf> {
    let home = env::var_os("HOME")?;
    let root = PathBuf::from(home).join(".local/share/icp-cli/pkg/network-launcher");
    let entries = fs::read_dir(root).ok()?;
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("pocket-ic"))
        .find(|path| validate_pocketic_binary(path).is_ok())
}

fn ensure_pocketic_bin_env() -> Result<()> {
    if let Some(path) = env::var_os("POCKET_IC_BIN") {
        validate_pocketic_binary(Path::new(&path))?;
        return Ok(());
    }
    if let Some(path) = discover_pocketic_binary() {
        eprintln!(
            "{DIM}Using PocketIC server {} at {}{RESET}",
            POCKET_IC_SERVER_VERSION,
            path.display()
        );
        env::set_var("POCKET_IC_BIN", path);
        return Ok(());
    }
    bail!(
        "could not find a local PocketIC server {POCKET_IC_SERVER_VERSION} binary; \
         install one or set POCKET_IC_BIN to an executable \
         `pocket-ic-server {POCKET_IC_SERVER_VERSION}` binary"
    );
}

pub(crate) fn pocketic_test_env() -> Result<[(&'static str, &'static str); 2]> {
    ensure_pocketic_bin_env()?;
    Ok([("POCKET_IC_MUTE_SERVER", "1"), ("RUST_TEST_THREADS", "1")])
}
