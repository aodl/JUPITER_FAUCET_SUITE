#!/usr/bin/env python3
"""Prepare local-only launch inputs; provision the hash-pinned official validator."""
import argparse
import base64
import gzip
import hashlib
import json
from pathlib import Path
import urllib.request
import subprocess

import economics

REPO = Path(__file__).resolve().parents[2]


def validator(fetch=False):
    pin = json.loads((REPO / 'deploy/acceptance/launch-tools.json').read_text())
    directory = Path.home() / '.cache/jupiter-launch' / pin['revision']
    directory.mkdir(parents=True, exist_ok=True)
    compressed = directory / 'sns.gz'
    if not compressed.exists():
        if not fetch:
            raise ValueError('official sns validator missing; explicit --fetch required')
        url = f"https://download.dfinity.systems/ic/{pin['revision']}/binaries/x86_64-linux/sns.gz"
        with urllib.request.urlopen(url, timeout=180) as reply:
            data = reply.read()
        if hashlib.sha256(data).hexdigest() != pin['sns_cli_gzip_sha256']:
            raise ValueError('official sns validator hash mismatch')
        compressed.write_bytes(data)
    if hashlib.sha256(compressed.read_bytes()).hexdigest() != pin['sns_cli_gzip_sha256']:
        raise ValueError('cached official sns validator hash mismatch')
    binary = directory / 'sns'
    binary.write_bytes(gzip.decompress(compressed.read_bytes()))
    binary.chmod(0o755)
    return binary


def candidate(directory):
    directory = Path(directory)
    directory.mkdir(exist_ok=True)
    fixture = json.loads((REPO / 'tests/pocketic/fixtures/jup-sns-genesis.json').read_text())
    logo = directory / 'logo.png'
    logo.write_bytes(base64.b64decode(fixture['metadata']['logo'].split(',')[1]))
    # Distinct well-known principals are fixture role labels, never live wiring.
    value = economics.launch_candidate('rrkah-fqaaa-aaaaa-aaaaq-cai',
        'ryjl3-tyaaa-aaaaa-aaaba-cai', 'r7inp-6aaaa-aaaaa-aaabq-cai',
        'rkp4c-7iaaa-aaaaa-aaaca-cai', logo.resolve())
    path = directory / 'jupiter.yaml'
    path.write_text(json.dumps(value, indent=2) + '\n')
    return path


def sns_wasm(fetch=False):
    pin = json.loads((REPO / 'deploy/acceptance/launch-tools.json').read_text())
    directory = validator(fetch).parent
    compressed = directory / 'sns-wasm-canister.wasm.gz'
    if not compressed.exists():
        if not fetch:
            raise ValueError('official SNS-W missing; explicit --fetch required')
        url = f"https://download.dfinity.systems/ic/{pin['revision']}/canisters/sns-wasm-canister.wasm.gz"
        with urllib.request.urlopen(url, timeout=180) as reply:
            data = reply.read()
        if hashlib.sha256(data).hexdigest() != pin['sns_wasm_gzip_sha256']:
            raise ValueError('official SNS-W hash mismatch')
        compressed.write_bytes(data)
    if hashlib.sha256(compressed.read_bytes()).hexdigest() != pin['sns_wasm_gzip_sha256']:
        raise ValueError('cached SNS-W hash mismatch')
    raw = directory / 'sns-wasm.wasm'
    raw.write_bytes(gzip.decompress(compressed.read_bytes()))
    did = subprocess.check_output(['ic-wasm', str(raw), 'metadata', 'candid:service'])[:-1]
    revision = subprocess.check_output(['ic-wasm', str(raw), 'metadata', 'git_commit_id']).decode().strip()
    if hashlib.sha256(did).hexdigest() != pin['sns_wasm_did_sha256'] or revision != pin['revision']:
        raise ValueError('SNS-W interface/source revision differs from pin')
    (directory / 'sns-wasm.did').write_bytes(did)
    return raw


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fetch', action='store_true')
    parser.add_argument('--candidate-directory')
    parser.add_argument('--sns-wasm', action='store_true')
    args = parser.parse_args()
    if args.sns_wasm:
        print(sns_wasm(args.fetch))
    elif args.candidate_directory:
        print(candidate(args.candidate_directory))
    else:
        print(validator(args.fetch))
