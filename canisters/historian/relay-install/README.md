# Retained Relay installation identity

`legacy-relay.wasm.gz` is the exact canonical Relay payload embedded in the predecessor Historian package with installed-module SHA-256 `8c673c582964b17c63fbcd9be496a35759f41e0e37549b6fb47ebbaf7523dd0e`.

- Compressed payload SHA-256: `221765e5940e3d0721dac7677f0294e05f616a6d91385f984c59a34ceb71cde0`.
- Decompressed Wasm SHA-256: `8db17b838a57e67c733b4d5d2ff098e07242e9d41784c35396f834ba0991b661`.
- Provenance: retained `baseline-release-artifacts` from the canonical acceptance handoff. The package's Historian hash matched the live certificate-checked observation, and its decompressed bytes contain this exact gzip payload. The source release pin is recorded at commit `5f162a6`.

It is retained solely to finish configurations paid under that installation identity. New factory configurations use the normal canonical embedded Relay. Never replace this file with a local development build. See [recovery and release procedure](../../../docs/relay-setup-recovery.md).
