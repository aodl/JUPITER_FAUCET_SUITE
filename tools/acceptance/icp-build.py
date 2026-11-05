#!/usr/bin/env python3
"""icp build handoff only; all installations remain separately authorized."""
import json
import hashlib
import os
from pathlib import Path
import subprocess
import sys

import profile as acceptance
import sns


def main(role):
    if os.environ.get('ICP_CLI_ENVIRONMENT', os.environ.get('ICP_ENVIRONMENT')) != 'acceptance':
        raise ValueError('acceptance artifacts require the acceptance CLI environment')
    source = Path(os.environ.get('JUPITER_ACCEPTANCE_PROFILE', acceptance.PROFILE))
    profile = json.loads(source.read_text())
    checked = acceptance.validate(profile)
    if profile['genesis'] is None or profile['genesis'].get('purpose') != 'reviewed-acceptance':
        raise ValueError('reviewed jUP genesis required; fixture/missing economics cannot install')
    mapping = acceptance.REPO / '.icp/data/mappings/acceptance.ids.json'
    if json.loads(mapping.read_text()) != checked['roles']:
        raise ValueError('acceptance CLI mapping differs from the reviewed profile')
    group = 'sns' if role.startswith('sns_') else 'applications'
    constructor_name = role.removeprefix('sns_') if group == 'sns' else role
    prepared = acceptance.REPO / 'target/acceptance/constructors' / group
    if json.loads((prepared / 'profile.json').read_text()) != profile:
        raise ValueError('prepared constructor profile differs from the build profile')
    manifest = json.loads((prepared / 'sha256.json').read_text())
    for name in ('profile.json', constructor_name + '.bin'):
        if hashlib.sha256((prepared / name).read_bytes()).hexdigest() != manifest[name]:
            raise ValueError('prepared constructor hash mismatch')
    if role.startswith('sns_'):
        directory = Path(os.environ['JUPITER_SNS_ARTIFACT_DIR'])
        lock = json.loads((acceptance.REPO / 'deploy/acceptance/sns-framework.json').read_text())
        sns.verify_artifacts(lock, directory)
        payload = directory / lock['components'][role.removeprefix('sns_')]['file']
    else:
        directory = acceptance.REPO / 'target/acceptance/artifacts'
        subprocess.run(['python3', 'tools/acceptance/build.py', str(source), str(directory), role], check=True)
        payload = directory / (role + '.wasm.gz')
    Path(os.environ['ICP_WASM_OUTPUT_PATH']).write_bytes(payload.read_bytes())


if __name__ == '__main__':
    try:
        main(sys.argv[1])
    except (ValueError, KeyError, OSError, IndexError) as error:
        sys.exit(f'acceptance CLI build rejected: {error}')
