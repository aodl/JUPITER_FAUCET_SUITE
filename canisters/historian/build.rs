fn main() {
    jupiter_build_support::emit_prod_canister_id(
        "JUPITER_HISTORIAN_PROD_CANISTER_ID",
        "jupiter_historian",
    );
    println!("cargo:rerun-if-env-changed=JUPITER_RELAY_WASM_PATH");
    let out_dir = std::env::var_os("OUT_DIR").expect("OUT_DIR not set");
    let out_path = std::path::PathBuf::from(out_dir).join("self_service_relay.wasm");
    if let Some(path) = std::env::var_os("JUPITER_RELAY_WASM_PATH") {
        std::fs::copy(&path, &out_path).unwrap_or_else(|err| {
            panic!(
                "failed to copy JUPITER_RELAY_WASM_PATH={} to {}: {err}",
                std::path::PathBuf::from(path).display(),
                out_path.display()
            )
        });
    } else {
        std::fs::write(&out_path, []).unwrap_or_else(|err| {
            panic!(
                "failed to write empty relay wasm marker {}: {err}",
                out_path.display()
            )
        });
    }
}
