"""Small off-chain IC principal and Account encodings; no project economics."""
import base64
import hashlib
import zlib

def principal_text(raw):
    data = zlib.crc32(raw).to_bytes(4, 'big') + raw
    text = base64.b32encode(data).decode().lower().rstrip('=')
    return '-'.join(text[i:i + 5] for i in range(0, len(text), 5))


def principal_bytes(text):
    if not isinstance(text, str):
        raise ValueError('unresolved principal')
    compact = text.replace('-', '').upper()
    try:
        encoded = base64.b32decode(compact + '=' * ((-len(compact)) % 8))
    except (ValueError, base64.binascii.Error) as error:
        raise ValueError(f'invalid principal: {text}') from error
    raw = encoded[4:]
    if len(encoded) < 4 or len(raw) > 29 or principal_text(raw) != text:
        raise ValueError(f'noncanonical or invalid principal: {text}')
    return raw


def staking_subaccount(controller, memo):
    return hashlib.sha256(b'\x0cneuron-stake' + principal_bytes(controller)
                          + memo.to_bytes(8, 'big')).hexdigest()


def account(owner, subaccount=None):
    raw_owner = principal_bytes(owner)
    if subaccount is not None and (not isinstance(subaccount, str) or len(subaccount) != 64
                                  or any(c not in '0123456789abcdefABCDEF' for c in subaccount)):
        raise ValueError('subaccountHex must contain exactly 64 hexadecimal characters')
    sub = bytes(32) if subaccount is None else bytes.fromhex(subaccount)
    if len(sub) != 32:
        raise ValueError('subaccount must have exactly 32 bytes')
    digest = hashlib.sha224(b'\x0aaccount-id' + raw_owner + sub).digest()
    legacy = (zlib.crc32(digest).to_bytes(4, 'big') + digest).hex()
    address = owner
    if any(sub):
        checksum = zlib.crc32(raw_owner + sub).to_bytes(4, 'big')
        suffix = base64.b32encode(checksum).decode().lower().rstrip('=')
        address += '-' + suffix + '.' + sub.hex().lstrip('0')
    return {'owner': owner, 'subaccountHex': None if subaccount is None else sub.hex(),
            'address': address, 'explorerAccountHex': legacy}
