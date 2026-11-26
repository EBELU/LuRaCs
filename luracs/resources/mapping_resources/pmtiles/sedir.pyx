
# cython: language_level=3
# cython: boundscheck=False
# cython: wraparound=False
# cython: initializedcheck=False
# cython: cdivision=True

import gzip
import io

from libc.stdint cimport uint64_t, int64_t
from libc.stdint cimport uint32_t
from functools import lru_cache


cdef class Entry:
    cdef public uint64_t tile_id
    cdef public uint64_t offset
    cdef public uint64_t length
    cdef public uint64_t run_length

    def __init__(
        self,
        uint64_t tile_id,
        uint64_t offset,
        uint64_t length,
        uint64_t run_length
    ):
        self.tile_id = tile_id
        self.offset = offset
        self.length = length
        self.run_length = run_length

    def __str__(self):
        return (
            f"id={self.tile_id} "
            f"offset={self.offset} "
            f"length={self.length} "
            f"runlength={self.run_length}"
        )


cdef inline void rotate(
    uint64_t n,
    uint64_t* x,
    uint64_t* y,
    uint64_t rx,
    uint64_t ry
) noexcept nogil:
    cdef uint64_t tmp

    if ry == 0:
        if rx != 0:
            x[0] = n - 1 - x[0]
            y[0] = n - 1 - y[0]

        tmp = x[0]
        x[0] = y[0]
        y[0] = tmp


cpdef uint64_t zxy_to_tileid(
    uint32_t z,
    uint64_t x,
    uint64_t y
):
    cdef uint64_t acc
    cdef uint64_t a
    cdef uint64_t s
    cdef uint64_t rx
    cdef uint64_t ry

    if z > 31:
        raise OverflowError("tile zoom exceeds 64-bit limit")

    if x >= (1 << z) or y >= (1 << z):
        raise ValueError("tile x/y outside zoom level bounds")

    acc = ((1 << (z * 2)) - 1) // 3
    a = z

    while a > 0:
        a -= 1
        s = 1 << a

        rx = x & s
        ry = y & s

        acc += ((3 * rx) ^ ry) << a

        rotate(s, &x, &y, rx, ry)

    return acc


cpdef tuple tileid_to_zxy(uint64_t tile_id):
    cdef uint32_t z
    cdef uint64_t acc
    cdef uint64_t pos
    cdef uint64_t x = 0
    cdef uint64_t y = 0
    cdef uint64_t s = 1
    cdef uint64_t n
    cdef uint64_t rx
    cdef uint64_t ry

    z = ((3 * tile_id + 1).bit_length() - 1) // 2

    if z >= 32:
        raise OverflowError("tile zoom exceeds 64-bit limit")

    acc = ((1 << (z * 2)) - 1) // 3
    pos = tile_id - acc

    n = 1 << z

    while s < n:
        rx = (pos // 2) & s
        ry = (pos ^ rx) & s

        rotate(s, &x, &y, rx, ry)

        x += rx
        y += ry

        pos >>= 1
        s <<= 1

    return z, x, y


cpdef Entry find_tile(list entries, uint64_t tile_id):
    cdef Py_ssize_t m = 0
    cdef Py_ssize_t n = len(entries) - 1
    cdef Py_ssize_t k
    cdef Entry entry

    while m <= n:
        k = (n + m) >> 1
        entry = entries[k]

        if tile_id > entry.tile_id:
            m = k + 1
        elif tile_id < entry.tile_id:
            n = k - 1
        else:
            return entry

    if n >= 0:
        entry = entries[n]

        if entry.run_length == 0:
            return entry

        if tile_id - entry.tile_id < entry.run_length:
            return entry

    return None


cpdef uint64_t read_varint(object b_io):
    cdef uint64_t result = 0
    cdef unsigned int shift = 0
    cdef bytes raw
    cdef unsigned char value

    while True:
        raw = b_io.read(1)

        if not raw:
            raise EOFError(
                "unexpectedly reached end of varint stream"
            )

        value = raw[0]

        if shift >= 64:
            raise OverflowError("varint exceeds 64-bit limit")

        result |= (<uint64_t>(value & 0x7F)) << shift

        if not (value & 0x80):
            return result

        shift += 7


cpdef write_varint(object b_io, uint64_t value):
    cdef unsigned char byte

    while True:
        byte = value & 0x7F
        value >>= 7

        if value:
            b_io.write(bytes([byte | 0x80]))
        else:
            b_io.write(bytes([byte]))
            return


@lru_cache(maxsize=16)
def deserialize_directory(bytes buf):
    cdef object b_io
    cdef list entries
    cdef Py_ssize_t num_entries
    cdef Py_ssize_t i
    cdef uint64_t last_id = 0
    cdef uint64_t tmp
    cdef Entry entry

    b_io = io.BytesIO(gzip.decompress(buf))
    entries = []

    num_entries = read_varint(b_io)

    for i in range(num_entries):
        tmp = read_varint(b_io)
        last_id += tmp

        entries.append(Entry(last_id, 0, 0, 0))

    for i in range(num_entries):
        entries[i].run_length = read_varint(b_io)

    for i in range(num_entries):
        entries[i].length = read_varint(b_io)

    for i in range(num_entries):
        tmp = read_varint(b_io)
        entry = entries[i]

        if i > 0 and tmp == 0:
            entry.offset = (
                entries[i - 1].offset +
                entries[i - 1].length
            )
        else:
            entry.offset = tmp - 1

    return entries


cpdef bytes serialize_directory(object entries):
    cdef object b_io = io.BytesIO()
    cdef Py_ssize_t num_entries = len(entries)
    cdef Py_ssize_t i
    cdef uint64_t last_id = 0
    cdef Entry entry

    write_varint(b_io, num_entries)

    for i in range(num_entries):
        entry = entries[i]
        write_varint(b_io, entry.tile_id - last_id)
        last_id = entry.tile_id

    for i in range(num_entries):
        write_varint(b_io, entries[i].run_length)

    for i in range(num_entries):
        write_varint(b_io, entries[i].length)

    for i in range(num_entries):
        entry = entries[i]

        if (
            i > 0 and
            entry.offset ==
            entries[i - 1].offset + entries[i - 1].length
        ):
            write_varint(b_io, 0)
        else:
            write_varint(b_io, entry.offset + 1)

    return gzip.compress(b_io.getvalue())
