pub(crate) const LOCAL_IDENTITY: &str = "xtask-dev";
pub(crate) const LOCAL_ENVIRONMENT: &str = "local";
pub(crate) const POCKET_IC_SERVER_VERSION: &str = "15.0.0";
pub(crate) const NETWORK_LAUNCHER_PACKAGE: &str = "15.0.0-2026-08-20-03-30";

pub(crate) const RESET: &str = "\x1b[0m";
pub(crate) const BOLD: &str = "\x1b[1m";
pub(crate) const GREEN: &str = "\x1b[32m";
pub(crate) const RED: &str = "\x1b[31m";
pub(crate) const DIM: &str = "\x1b[2m";

#[cfg(test)]
mod tests {
    use super::NETWORK_LAUNCHER_PACKAGE;

    fn local_network_version(config: &str) -> Option<&str> {
        let mut in_local_network = false;
        for line in config.lines() {
            let trimmed = line.trim();
            if trimmed == "- name: local" {
                in_local_network = true;
                continue;
            }
            if in_local_network && trimmed.starts_with("- name:") {
                return None;
            }
            if in_local_network {
                if let Some(version) = trimmed.strip_prefix("version:") {
                    return Some(version.trim());
                }
            }
        }
        None
    }

    fn pocketic_support_launcher_package(source: &str) -> Option<&str> {
        source.lines().find_map(|line| {
            let value = line
                .trim()
                .strip_prefix("const NETWORK_LAUNCHER_PACKAGE: &str = ")?;
            value.strip_prefix('"')?.strip_suffix("\";")
        })
    }

    #[test]
    fn managed_network_and_pocketic_launcher_package_pins_match() {
        let icp_yaml = include_str!("../../../icp.yaml");
        let pocketic_support = include_str!("../../../tests/pocketic/support/pocketic.rs");

        assert_eq!(
            local_network_version(icp_yaml),
            Some(NETWORK_LAUNCHER_PACKAGE),
            "icp.yaml local managed-network launcher must match xtask"
        );
        assert_eq!(
            pocketic_support_launcher_package(pocketic_support),
            Some(NETWORK_LAUNCHER_PACKAGE),
            "PocketIC binary discovery launcher must match xtask"
        );
    }
}
