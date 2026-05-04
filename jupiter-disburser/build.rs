use std::{env, fs, path::PathBuf};

use serde_json::Value;

fn load_ic_canister_id(mapping_path: &PathBuf, json_key: &str) -> String {
    let contents = fs::read_to_string(mapping_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {}", mapping_path.display(), err));
    let value: Value = serde_json::from_str(&contents)
        .unwrap_or_else(|err| panic!("failed to parse {} as JSON: {}", mapping_path.display(), err));

    value
        .get(json_key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            panic!(
                "missing ic canister id for {} in {}",
                json_key,
                mapping_path.display()
            )
        })
}

fn main() {
    println!("cargo:rerun-if-env-changed=JUPITER_DISBURSER_PROD_CANISTER_ID");
    println!("cargo:rerun-if-changed=../.icp/data/mappings/ic.ids.json");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let mapping_path = manifest_dir.join("../.icp/data/mappings/ic.ids.json");
    let canonical_prod_id = load_ic_canister_id(&mapping_path, "jupiter_disburser");

    if let Ok(value) = env::var("JUPITER_DISBURSER_PROD_CANISTER_ID") {
        let trimmed = value.trim();
        if !trimmed.is_empty() && trimmed != canonical_prod_id {
            panic!(
                "{} override mismatch: expected '{}' from {} but got '{}'",
                "JUPITER_DISBURSER_PROD_CANISTER_ID",
                canonical_prod_id,
                mapping_path.display(),
                trimmed
            );
        }
    }

    println!(
        "cargo:rustc-env=JUPITER_DISBURSER_PROD_CANISTER_ID={}",
        canonical_prod_id
    );
}
