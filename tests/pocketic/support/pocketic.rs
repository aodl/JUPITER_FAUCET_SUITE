use pocket_ic::{start_server, PocketIcBuilder, StartServerParams};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

const SERVER_HARD_TTL_SECS: u64 = 3 * 60 * 60;
const SERVER_VERSION: &str = "13.0.0";

static SERVER_URL: OnceLock<String> = OnceLock::new();

fn validate_pocketic_binary(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() || metadata.len() == 0 {
        return false;
    }
    let Ok(output) = Command::new(path).arg("--version").output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let version = String::from_utf8_lossy(&output.stdout);
    let expected = format!("pocket-ic-server {SERVER_VERSION}");
    version.trim() == expected
}

fn discover_pocketic_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("POCKET_IC_BIN").map(PathBuf::from) {
        if validate_pocketic_binary(&path) {
            return Some(path);
        }
        panic!(
            "POCKET_IC_BIN must point to an executable `pocket-ic-server {SERVER_VERSION}` binary"
        );
    }

    let home = std::env::var_os("HOME")?;
    let root = PathBuf::from(home).join(".local/share/icp-cli/pkg/network-launcher");
    std::fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("pocket-ic"))
        .find(|path| validate_pocketic_binary(path))
}

pub fn builder() -> PocketIcBuilder {
    let server_url = SERVER_URL.get_or_init(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .expect("failed to create PocketIC server runtime");
        let server_binary = discover_pocketic_binary()
            .expect("failed to find a local executable pocket-ic-server 13.0.0 binary");
        let (_, url) = runtime.block_on(start_server(StartServerParams {
            server_binary: Some(server_binary),
            reuse: true,
            hard_ttl: Some(Duration::from_secs(SERVER_HARD_TTL_SECS)),
            ..Default::default()
        }));
        url.to_string()
    });

    PocketIcBuilder::new().with_server_url(
        server_url
            .parse()
            .expect("PocketIC server URL from start_server should parse"),
    )
}
