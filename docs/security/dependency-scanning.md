# Dependency Scanning

Jupiter Faucet release checks include Rust advisory scanning, Rust license/source policy checks, npm advisory scanning, OSV lockfile scanning, and CycloneDX SBOM generation.

## Tool Setup

Release security evidence uses pinned scanner versions so SBOMs and advisory output
are reproducible across release reruns. Install the Rust scanners with the
checked-in Rust toolchain:

```bash
cargo install cargo-audit --version 0.22.1 --locked
cargo install cargo-deny --version 0.19.8 --locked
cargo install cargo-cyclonedx --version 0.5.9 --locked
```

Install Node dependencies from the committed lockfile:

```bash
npm ci
```

Install OSV Scanner from the official GitHub release binary. The version observed while adding this gate was `v2.3.8`:

```bash
mkdir -p "$HOME/bin"
curl -fsSLo /tmp/osv-scanner_linux_amd64 https://github.com/google/osv-scanner/releases/download/v2.3.8/osv-scanner_linux_amd64
curl -fsSLo /tmp/osv-scanner_SHA256SUMS https://github.com/google/osv-scanner/releases/download/v2.3.8/osv-scanner_SHA256SUMS
grep 'osv-scanner_linux_amd64$' /tmp/osv-scanner_SHA256SUMS | (cd /tmp && sha256sum -c -)
install -m 0755 /tmp/osv-scanner_linux_amd64 "$HOME/bin/osv-scanner"
```

The npm SBOM step uses `npx --yes @cyclonedx/cyclonedx-npm@4.2.1` so the
CycloneDX npm CLI is provisioned by npm when it is not already cached, while
remaining pinned for release evidence.

## Local Gate

Run the canonical local command from any directory in the repository:

```bash
./tools/scripts/security-scan
```

The script fails non-zero if any of these steps fails:

- `cargo audit`
- `cargo deny check advisories licenses bans sources`
- `npm audit --omit=dev`
- `osv-scanner scan -L Cargo.lock -L package-lock.json`
- Rust or npm SBOM generation

The script derives deterministic SBOM timestamps and serial numbers from
`SOURCE_DATE_EPOCH`. If `SOURCE_DATE_EPOCH` is unset, it uses the Unix timestamp
of the current `HEAD` commit (`git log -1 --pretty=%ct`). Override
`SOURCE_DATE_EPOCH` only when intentionally reproducing evidence for another
source date.

## Release Artifacts

The security gate writes CycloneDX SBOMs and hashes to:

```text
release-artifacts/sbom/cargo.cdx.json
release-artifacts/sbom/npm.cdx.json
release-artifacts/sbom/SHA256SUMS
```

Keep advisory and license exceptions narrow. Any exception in `deny.toml` must explain why it is acceptable for production and whether the dependency is runtime, build-time, test-only, or dev-only.
OSV exceptions live in `osv-scanner.toml` because OSV Scanner does not read `deny.toml`; keep the two files aligned for shared RustSec findings.
