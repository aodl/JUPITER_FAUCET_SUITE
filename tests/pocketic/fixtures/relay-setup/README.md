# Frozen predecessor writer

`historian-2026-10-24.wasm.gz` is the canonical no-debug Historian package retained from the acceptance baseline. Its SHA-256 is `8c673c582964b17c63fbcd9be496a35759f41e0e37549b6fb47ebbaf7523dd0e`, matching the installed Historian observed during the Relay incident investigation.

The PocketIC regression runs this writer with its 2 trillion child allocation on a 34-node fiduciary subnet, preserves the old paid failure and unrelated entries in memories 26/30, then upgrades in place and exercises public continuation. It also interrupts the supplemental conversion before another upgrade. The Relay payload retained under `canisters/historian/relay-install/` occurs byte for byte inside this decompressed writer.
