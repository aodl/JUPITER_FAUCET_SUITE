fn main() {
    jupiter_build_support::emit_prod_canister_id(
        "JUPITER_FAUCET_PROD_CANISTER_ID",
        "jupiter_faucet",
    );
    jupiter_build_support::emit_prod_canister_id("JUPITER_RELAY_PROD_CANISTER_ID", "jupiter_relay");
}
