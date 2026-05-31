#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TestComponent {
    Test,
    Disburser,
    Faucet,
    Historian,
    Relay,
    Frontend,
    E2e,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TestScope {
    Unit,
    LocalIntegration,
    PocketicIntegration,
    All,
}

pub(crate) fn parse_scoped_command(cmd: &str) -> Option<(TestComponent, TestScope)> {
    use TestComponent::{Disburser, E2e, Faucet, Frontend, Historian, Relay, Test};
    use TestScope::{All, LocalIntegration, PocketicIntegration, Unit};

    match cmd {
        "disburser_unit" => Some((Disburser, Unit)),
        "disburser_local_integration" => Some((Disburser, LocalIntegration)),
        "disburser_pocketic_integration" => Some((Disburser, PocketicIntegration)),
        "disburser_all" => Some((Disburser, All)),
        "faucet_unit" => Some((Faucet, Unit)),
        "faucet_local_integration" => Some((Faucet, LocalIntegration)),
        "faucet_pocketic_integration" => Some((Faucet, PocketicIntegration)),
        "faucet_all" => Some((Faucet, All)),
        "historian_unit" => Some((Historian, Unit)),
        "historian_local_integration" => Some((Historian, LocalIntegration)),
        "historian_pocketic_integration" => Some((Historian, PocketicIntegration)),
        "historian_all" => Some((Historian, All)),
        "relay_unit" => Some((Relay, Unit)),
        "relay_local_integration" => Some((Relay, LocalIntegration)),
        "relay_pocketic_integration" => Some((Relay, PocketicIntegration)),
        "relay_all" => Some((Relay, All)),
        "frontend_unit" => Some((Frontend, Unit)),
        "frontend_local_integration" => Some((Frontend, LocalIntegration)),
        "frontend_all" => Some((Frontend, All)),
        "e2e_all" => Some((E2e, All)),
        "e2e_pocketic_integration" => Some((E2e, PocketicIntegration)),
        "test_unit" => Some((Test, Unit)),
        "test_local_integration" => Some((Test, LocalIntegration)),
        "test_pocketic_integration" => Some((Test, PocketicIntegration)),
        "test_all" => Some((Test, All)),
        _ => None,
    }
}
