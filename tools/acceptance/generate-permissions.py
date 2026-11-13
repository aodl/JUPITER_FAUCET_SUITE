#!/usr/bin/env python3
"""Regenerate named enum constants from the exact reviewed official protobuf."""
import hashlib
from pathlib import Path
import re
import sys

REVISION = '98c898f8de068a1fc0e97ebc805f9fa7f315d862'
SOURCE_SHA256 = 'c8135574513ec10a9542079655b3fc85e0033f7c7b689bf0ab33a527927179f8'


def generate(source):
    if hashlib.sha256(source).hexdigest() != SOURCE_SHA256:
        raise ValueError('official governance.proto differs from reviewed source')
    block = re.search(r'enum NeuronPermissionType \{(.*?)\n\}', source.decode(), re.S)[1]
    variants = re.findall(r'NEURON_PERMISSION_TYPE_(\w+) = (\d+);', block)
    entries = [(''.join(x.title() for x in name.split('_')), number) for name, number in variants]
    text = (f'// Generated from official governance.proto at {REVISION}.\n'
            f'// Source SHA-256: {SOURCE_SHA256}\n'
            '// Candid represents these protobuf enum values as int32.\n'
            '#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n#[repr(i32)]\n'
            'pub enum NeuronPermissionType {\n')
    text += ''.join(f'    {name} = {number},\n' for name, number in entries)
    text += ('}\nimpl TryFrom<i32> for NeuronPermissionType {\n    type Error = ();\n'
             '    fn try_from(value: i32) -> Result<Self, Self::Error> {\n'
             '        match value {\n')
    text += ''.join(f'            {number} => Ok(Self::{name}),\n' for name, number in entries)
    not_found = re.search(r'ERROR_TYPE_NOT_FOUND = (\d+);', source.decode())[1]
    return text + '            _ => Err(()),\n        }\n    }\n}\n\n' + f'pub const GOVERNANCE_ERROR_NOT_FOUND: i32 = {not_found};\n'


if __name__ == '__main__':
    source, output = sys.argv[1:]
    Path(output).write_text(generate(Path(source).read_bytes()))
