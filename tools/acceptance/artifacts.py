#!/usr/bin/env python3
"""Provision an exact official SNS tuple in a content-addressed local cache."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import sys
import urllib.request

import sns


def provision(lock_path, directory=None, fetch=False):
    source = Path(lock_path).read_bytes()
    lock = json.loads(source)
    out = Path(directory) if directory else Path.home() / '.cache/verified-sns' / hashlib.sha256(source).hexdigest()[:16]
    out.mkdir(parents=True, exist_ok=True)
    for role, pin in lock['components'].items():
        compressed = out / (role + '.wasm.gz')
        raw = out / pin['file']
        if not compressed.exists():
            if not fetch:
                raise ValueError(f'missing {compressed}; explicit --fetch required')
            expected_prefix = 'https://download.dfinity.systems/ic/' + pin['revision'] + '/canisters/'
            if not pin['url'].startswith(expected_prefix):
                raise ValueError('only exact official source URLs are allowed')
            with urllib.request.urlopen(pin['url'], timeout=120) as response:
                data = response.read()
            if sns.sha(data) != pin['gzip_sha256']:
                raise ValueError(f'{role}: downloaded bytes differ from reviewed hash')
            compressed.write_bytes(data)
        data = compressed.read_bytes()
        if sns.sha(data) != pin['gzip_sha256']:
            raise ValueError(f'{role}: cached compressed bytes differ from reviewed hash')
        decoded = gzip.decompress(data)
        if sns.sha(decoded) != pin['raw_sha256']:
            raise ValueError(f'{role}: decoded Wasm differs from reviewed hash')
        if not raw.exists():
            raw.write_bytes(decoded)
    sns.verify_artifacts(lock, out)
    return out.resolve()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--lock', default='deploy/acceptance/sns-framework.json')
    parser.add_argument('--directory')
    parser.add_argument('--fetch', action='store_true')
    args = parser.parse_args()
    try:
        print(provision(args.lock, args.directory, args.fetch))
    except (ValueError, OSError) as error:
        sys.exit(f'official SNS inputs rejected: {error}')
