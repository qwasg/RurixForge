"""Bind a signed PE to the exact unsigned file with a narrow allowed change set.

This is not an implementation of Windows signature validation or the complete
Authenticode hashing algorithm. The separate PowerShell check must validate the
real signature. Here only checksum/security-directory changes, zero alignment
padding and one appended WIN_CERTIFICATE are allowed; all original other bytes
must match. Other legitimate signing layouts fail closed for manual review.
"""
from pathlib import Path
import argparse
import hashlib
import json
import struct


def require(condition, message):
    if not condition:
        raise ValueError(message)


def layout(data):
    require(len(data) >= 64 and data[:2] == b'MZ', 'Not a DOS/PE image.')
    pe = struct.unpack_from('<I', data, 0x3c)[0]
    require(pe + 24 <= len(data) and data[pe:pe+4] == b'PE\0\0', 'Invalid PE header.')
    require(struct.unpack_from('<H', data, pe+4)[0] == 0x8664, 'Only the reviewed AMD64 engine target is allowed.')
    optional = pe + 24
    size = struct.unpack_from('<H', data, pe+20)[0]
    require(optional + size <= len(data) and size >= 152, 'Truncated optional header.')
    require(struct.unpack_from('<H', data, optional)[0] == 0x20b, 'Only PE32+ is allowed.')
    require(struct.unpack_from('<I', data, optional+108)[0] >= 5, 'Certificate directory is absent.')
    checksum, directory = optional + 64, optional + 112 + 4*8
    certificate_offset, certificate_size = struct.unpack_from('<II', data, directory)
    return dict(checksumOffset=checksum, securityDirectoryOffset=directory,
                certificateOffset=certificate_offset, certificateSize=certificate_size)


def payload_sha(data, info):
    normalized = bytearray(data)
    normalized[info['checksumOffset']:info['checksumOffset']+4] = b'\0' * 4
    normalized[info['securityDirectoryOffset']:info['securityDirectoryOffset']+8] = b'\0' * 8
    return hashlib.sha256(normalized).hexdigest()


def unsigned_record(data):
    info = layout(data)
    require(info['certificateOffset'] == info['certificateSize'] == 0, 'Build artifact already contains a certificate table.')
    return dict(scheme='exact-original-bytes-except-checksum-and-security-directory-v1', unsignedBytes=len(data),
                checksumOffset=info['checksumOffset'], securityDirectoryOffset=info['securityDirectoryOffset'],
                unsignedPayloadSha256=payload_sha(data, info))


def verify_signed(data, expected):
    require(expected['scheme'] == 'exact-original-bytes-except-checksum-and-security-directory-v1', 'Unknown content binding scheme.')
    info = layout(data)
    original_size = expected['unsignedBytes']
    require(type(original_size) is int and original_size > info['securityDirectoryOffset'] + 8, 'Invalid expected unsigned length.')
    require(info['checksumOffset'] == expected['checksumOffset'] and info['securityDirectoryOffset'] == expected['securityDirectoryOffset'], 'PE header layout changed.')
    aligned = (original_size + 7) & ~7
    offset, size = info['certificateOffset'], info['certificateSize']
    require(offset == aligned and size >= 8 and offset + size == len(data), 'Signature must be a single appended certificate table after the original image.')
    require(not any(data[original_size:offset]), 'Only zero alignment padding is allowed before the certificate.')
    length, revision, certificate_type = struct.unpack_from('<IHH', data, offset)
    require(length >= 8 and ((length + 7) & ~7) == size and revision == 0x0200 and certificate_type == 0x0002,
            'Unexpected WIN_CERTIFICATE layout; require manual review.')
    require(not any(data[offset+length:offset+size]), 'Unexpected nonzero certificate alignment padding.')
    actual = payload_sha(data[:original_size], info)
    require(actual == expected['unsignedPayloadSha256'], 'Signed executable program bytes differ from the exact CI-built unsigned executable.')
    return dict(programContentMatchesUnsigned=True, bindingScheme=expected['scheme'], unsignedBytes=original_size,
                unsignedPayloadSha256=actual, certificateBytes=size, cryptographicSignatureValidatedHere=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=['verify'])
    parser.add_argument('--signed', type=Path, required=True)
    parser.add_argument('--provenance', type=Path, required=True)
    args = parser.parse_args()
    provenance = json.loads(args.provenance.read_text(encoding='utf-8'))
    print(json.dumps(verify_signed(args.signed.read_bytes(), provenance['signingContent'])), flush=True)


if __name__ == '__main__':
    main()
