# Jupiter Faucet

`jupiter-faucet` is currently deployed as a placeholder canister.

## Current mainnet canister

- canister id: `acjuz-liaaa-aaaar-qb4qq-cai`
- subnet: Fiduciary (`pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae`)

This canister is the configured normal recipient for `jupiter-disburser`.

The production intent is for `jupiter-faucet` to receive the age-neutral ICP flow, convert it into cycles, and top up participating canisters proportionally to the stake commitments encoded upstream.

Users or upstream protocol components may top up the neuron's staking account and identify a target canister by putting that canister id into the transfer memo / attribution flow, but users should not have to make a separate governance call to recognize stake. In the current architecture that periodic best-effort `ClaimOrRefresh` now lives in `jupiter-disburser`; this canister remains a stub until its own conversion / attribution logic is implemented.

Like `jupiter-disburser`, `jupiter-faucet` is intended to be blackholed once deployment has been validated. It will use the same `jupiter-lifeline` recovery pattern.

## Current implementation

The current implementation is intentionally minimal. It exists to reserve the canister principal and default account while the production faucet logic is built. Memo / attribution parsing, cycles conversion, and downstream top-up accounting are still to be implemented here; periodic best-effort neuron refresh currently happens in `jupiter-disburser`.

## Pending production requirements

Before `jupiter-faucet` stops being a stub, it still needs at least:

- top-up attribution coverage proving that user / protocol stake top-ups are recognized without a user governance call
- end-to-end coverage proving that, after protocol-driven refresh, later maturity growth is higher than before the top-up
- the memo / attribution path that maps a user-provided target canister id in the deposit memo to the canister they want topped up

## Upgrade command

No install or upgrade argument is currently required.

```bash
dfx canister install jupiter_faucet   --network ic   --mode upgrade   --wasm release-artifacts/jupiter_faucet.wasm.gz
```
