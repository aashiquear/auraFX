"""Rewrite a version-B ADF file (binary little-endian pointers) as an old version-A file:
hex-text pointers and dimensions, big-endian data, and every array of 2+ values split into
two data chunks through a chunk table appended at the end of the file."""
import struct, sys
src, dst = sys.argv[1], sys.argv[2]
b = bytearray(open(src, 'rb').read())
assert b[25:26] == b'B' and b[100:101] == b'L'
def rptr(p): return struct.unpack_from('<Q', b, p)[0] * 4096 + struct.unpack_from('<I', b, p + 8)[0]
def hexptr(pos):
    blk, off = divmod(pos, 4096)
    return f"{blk:08X}{off:04X}".encode()
def wptr(p, pos): b[p:p + 12] = hexptr(pos)
NULL = b'000000001000'
def is_null(p): return struct.unpack_from('<I', b, p + 8)[0] >= 4096
def fix(p):
    if is_null(p): b[p:p+12] = NULL; return None
    pos = rptr(p); wptr(p, pos); return pos
SIZES = {'I4': 'i', 'U4': 'I', 'R4': 'f', 'I8': 'q', 'U8': 'Q', 'R8': 'd'}
tail = bytearray()
def append(data):
    global tail
    pos = len(b) + len(tail); tail += data; return pos
done = set()
def node(pos):
    if pos in done: return
    done.add(pos)
    assert b[pos:pos+4] == b'NoDe'
    nd = int(b[pos+128:pos+130], 16)
    dims = struct.unpack_from('<12Q', b, pos + 130)
    b[pos+130:pos+226] = b''.join(f"{d:08X}".encode() for d in dims)
    dtype = b[pos+96:pos+128].decode().strip()
    table = fix(pos + 84)
    nch = int(b[pos+226:pos+230], 16)
    chunks = None if is_null(pos + 230) else rptr(pos + 230)
    if table is not None:
        end = rptr(table + 4); wptr(table + 4, end)
        e = table + 16
        while e < end:
            if not is_null(e + 32):
                child = rptr(e + 32); wptr(e + 32, child); node(child)
            else:
                b[e+32:e+44] = NULL
            e += 44
    if chunks is None:
        b[pos+230:pos+242] = NULL; return
    assert nch == 1
    end = rptr(chunks + 4)
    data = bytes(b[chunks + 16:end])
    if dtype in SIZES:
        f = SIZES[dtype]; n = len(data) // struct.calcsize(f)
        data = struct.pack('>%d%s' % (n, f), *struct.unpack('<%d%s' % (n, f), data))
        b[chunks+16:end] = data
    wptr(chunks + 4, end)
    n = 1
    for d in dims[:nd]: n *= d
    if dtype in SIZES and n >= 2:
        w = struct.calcsize(SIZES[dtype]); cut = (n // 2) * w
        starts = []
        for part in (data[:cut], data[cut:]):
            s = len(b) + len(tail)
            starts.append((s, s + 16 + len(part)))
            append(b'DaTa' + hexptr(s + 16 + len(part)) + part + b'dEnD')
        t = len(b) + len(tail)
        body = b''.join(hexptr(s) + hexptr(e) for s, e in starts)
        append(b'DCtb' + hexptr(t + 16 + len(body)) + body + b'dcTE')
        b[pos+226:pos+230] = b'0002'
        wptr(pos + 230, t)
    else:
        wptr(pos + 230, chunks)
root = rptr(134)
for p in (134, 146, 158, 170):
    fix(p)
node(root)
b[25:26] = b'A'; b[100:101] = b'B'
open(dst, 'wb').write(b + tail)
print("wrote", dst, len(b) + len(tail))
