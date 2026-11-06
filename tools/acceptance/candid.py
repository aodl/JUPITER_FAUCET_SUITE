"""Typed, round-trip-checked constructor serialization through the pinned didc."""
from pathlib import Path
import subprocess


def text(value):
    # Candid's Unicode escape syntax differs from JSON. Byte escapes also handle
    # quotes, slashes and control characters without changing UTF-8 text.
    return '"' + ''.join(chr(b) if 32 <= b < 127 and b not in (34, 92)
                         else f'\\{b:02x}' for b in value.encode('utf-8')) + '"'


def principal(value):
    return 'principal ' + text(value)


def blob(value):
    return 'blob "' + ''.join(f'\\{byte:02x}' for byte in value) + '"'


def record(**fields):
    return 'record { ' + '; '.join(text(key) + ' = ' + value for key, value in fields.items()) + ' }'


def vector(values):
    return 'vec { ' + '; '.join(values) + ' }'


def account(value):
    sub = value.get('subaccountHex')
    return record(owner=principal(value['owner']),
                  subaccount='null' if sub is None else 'opt ' + blob(bytes.fromhex(sub)))


def encode_checked(did, types, value):
    command = ['didc', 'encode', '--defs', str(did), '--types', types]
    encoded = bytes.fromhex(subprocess.check_output(command, input=value, text=True).strip())
    decoded = subprocess.check_output(
        ['didc', 'decode', '--defs', str(did), '--types', types, encoded.hex()], text=True)
    again = bytes.fromhex(subprocess.check_output(command, input=decoded, text=True).strip())
    if encoded != again:
        raise ValueError('typed constructor did not round trip exactly')
    return encoded


def write_constructor(directory, role, did, types, value):
    encoded = encode_checked(did, types, '(' + value + ')')
    Path(directory, role + '.did').write_text('(' + value + ')\n')
    Path(directory, role + '.bin').write_bytes(encoded)
    return encoded
