use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde_json::Value;

pub const IC_CANISTER_ID_MAPPING_RELATIVE_PATH: &str = "../.icp/data/mappings/ic.ids.json";

pub fn load_ic_canister_id(mapping_path: impl AsRef<Path>, json_key: &str) -> String {
    let mapping_path = mapping_path.as_ref();
    let contents = fs::read_to_string(mapping_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {}", mapping_path.display(), err));
    let value: Value = serde_json::from_str(&contents).unwrap_or_else(|err| {
        panic!(
            "failed to parse {} as JSON: {}",
            mapping_path.display(),
            err
        )
    });

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

pub fn emit_prod_canister_id(env_var: &str, json_key: &str) {
    println!("cargo:rerun-if-env-changed={env_var}");
    println!("cargo:rerun-if-changed={IC_CANISTER_ID_MAPPING_RELATIVE_PATH}");

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let mapping_path = manifest_dir.join(IC_CANISTER_ID_MAPPING_RELATIVE_PATH);
    let canonical_prod_id = load_ic_canister_id(&mapping_path, json_key);

    if let Ok(value) = env::var(env_var) {
        let trimmed = value.trim();
        if !trimmed.is_empty() && trimmed != canonical_prod_id {
            panic!(
                "{} override mismatch: expected '{}' from {} but got '{}'",
                env_var,
                canonical_prod_id,
                mapping_path.display(),
                trimmed
            );
        }
    }

    println!("cargo:rustc-env={env_var}={canonical_prod_id}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_mapping_path(test_name: &str) -> PathBuf {
        let mut path = env::temp_dir();
        path.push(format!(
            "jupiter-build-support-{test_name}-{}-{}.json",
            std::process::id(),
            std::thread::current().name().unwrap_or("thread")
        ));
        path
    }

    #[test]
    fn loads_trimmed_canister_id() {
        let path = temp_mapping_path("loads_trimmed_canister_id");
        fs::write(
            &path,
            r#"{ "jupiter_faucet": " ryjl3-tyaaa-aaaaa-aaaba-cai " }"#,
        )
        .unwrap();

        let id = load_ic_canister_id(&path, "jupiter_faucet");

        fs::remove_file(&path).unwrap();
        assert_eq!(id, "ryjl3-tyaaa-aaaaa-aaaba-cai");
    }

    #[test]
    fn rejects_missing_canister_id() {
        let path = temp_mapping_path("rejects_missing_canister_id");
        fs::write(&path, r#"{ "jupiter_faucet": "" }"#).unwrap();

        let result = std::panic::catch_unwind(|| load_ic_canister_id(&path, "jupiter_faucet"));

        fs::remove_file(&path).unwrap();
        assert!(result.is_err());
    }
}
