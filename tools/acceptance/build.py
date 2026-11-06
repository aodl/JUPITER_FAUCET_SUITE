#!/usr/bin/env python3
"""Local acceptance Wasms only. Outputs never replace release-artifacts/."""
import fcntl
import gzip
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

import profile as acceptance


def build(role, source, directory, debug=False):
    profile = json.loads(Path(source).read_text())
    acceptance.validate(profile)
    if debug and (profile['genesis'] is None or profile['genesis']['purpose'] != 'local-fixture-not-for-deployment'):
        raise ValueError('debug APIs are only built for an explicit local fixture')
    if role not in acceptance.APPLICATIONS:
        raise ValueError('unknown Jupiter application role')
    out = Path(directory).resolve()
    if out == (acceptance.REPO / 'release-artifacts').resolve():
        raise ValueError('acceptance artifacts must not replace canonical release artifacts')
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, JUPITER_BUILD_ENVIRONMENT='acceptance',
               JUPITER_ACCEPTANCE_PROFILE=str(Path(source).resolve()))
    package = role.replace('_', '-')
    if role == 'jupiter_historian':
        build('jupiter_relay', source, out)
        raw, compressed = out / 'jupiter_relay.wasm', out / 'jupiter_relay.wasm.gz'
        env.update(JUPITER_RELAY_RAW_WASM_PATH=str(raw), JUPITER_RELAY_WASM_PATH=str(compressed),
                   JUPITER_RELAY_RAW_WASM_SHA256=hashlib.sha256(raw.read_bytes()).hexdigest(),
                   JUPITER_RELAY_GZ_WASM_SHA256=hashlib.sha256(compressed.read_bytes()).hexdigest())
    public = acceptance.REPO / 'canisters/frontend/public'
    originals = {}
    try:
        if role == 'jupiter_faucet_frontend':
            subprocess.run(['npm', 'run', 'build:frontend'], env=env, check=True, cwd=acceptance.REPO)
            manifest = json.loads((public / 'generated/frontend-bundle.json').read_text())
            bundle = manifest['bundlePath']
            version = hashlib.sha256(Path(source).read_bytes() + (public / bundle).read_bytes())
            for path in sorted(public.rglob('*')):
                if not path.is_file():
                    continue
                try:
                    text = path.read_text()
                except UnicodeDecodeError:
                    continue
                if '__ASSET_VERSION__' in text or '__APP_BUNDLE_PATH__' in text or path.name == 'ic-domains':
                    originals[path] = path.read_bytes()
                    version.update(originals[path])
            token = 'acceptance-' + version.hexdigest()[:16]
            for path in originals:
                subprocess.run(['node', 'tools/scripts/frontend-render.mjs', str(path), token, bundle],
                               env=env, check=True, cwd=acceptance.REPO)
        command = ['cargo', 'build', '--locked', '--target', 'wasm32-unknown-unknown', '--release', '-p', package]
        if debug:
            command += ['--features', 'debug_api']
        subprocess.run(command, env=env, check=True, cwd=acceptance.REPO)
    finally:
        for path, original in originals.items():
            path.write_bytes(original)
    stem = role + ('.debug' if debug else '')
    raw = out / (stem + '.wasm')
    component = 'frontend' if role == 'jupiter_faucet_frontend' else role.removeprefix('jupiter_').replace('_', '-')
    did = acceptance.REPO / 'canisters' / component / (role + '.did')
    if debug:
        did = did.with_name(role + '_debug.did')
    subprocess.run(['ic-wasm', str(acceptance.REPO / 'target/wasm32-unknown-unknown/release' / (role + '.wasm')),
                    '-o', str(raw), 'metadata', 'candid:service', '-f', str(did), '-v', 'public'], check=True)
    subprocess.run(['ic-wasm', str(raw), '-o', str(raw), 'shrink'], check=True)
    with (out / (stem + '.wasm.gz')).open('wb') as file:
        with gzip.GzipFile(filename='', mode='wb', fileobj=file, mtime=0, compresslevel=9) as compressed:
            compressed.write(raw.read_bytes())
    record = {
        'role': role, 'environment': 'acceptance',
        'profile_sha256': hashlib.sha256(Path(source).read_bytes()).hexdigest(),
        'source_head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=acceptance.REPO, text=True).strip(),
        'source_dirty': bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=acceptance.REPO)),
        'raw_sha256': hashlib.sha256(raw.read_bytes()).hexdigest(),
        'gzip_sha256': hashlib.sha256((out / (stem + '.wasm.gz')).read_bytes()).hexdigest(),
        'canonical_release': False,
    }
    (out / (stem + '.build.json')).write_text(json.dumps(record, sort_keys=True, indent=2) + '\n')
    return raw


if __name__ == '__main__':
    try:
        source, output, role, *options = sys.argv[1:]
        if options not in ([], ['--debug-api']):
            raise ValueError('only --debug-api is supported for local fixtures')
        Path(output).mkdir(parents=True, exist_ok=True)
        with (Path(output) / '.build.lock').open('w') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            print(build(role, source, output, bool(options)))
    except (ValueError, OSError) as error:
        sys.exit(f'acceptance build rejected: {error}')
