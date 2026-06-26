# from numba import njit, types
import numpy as np


# @njit("int16(uint8, uint8)", cache=True)
def two_bytes_to_int(b1: np.uint8, b2: np.uint8) -> np.int16:
    return np.uint16(b2 & 0xFF) * 256 + (b1 & 0xFF)


# @njit("int32(uint8, uint8, uint8)", cache=True)
def three_bytes_to_int(b3: np.uint8, b2: np.uint8, b1: np.uint8) -> np.int32:
    return np.uint32(np.uint32(b3) | (np.uint32(b2) << 8) | (np.uint32(b1) << 16))


def four_bytes_to_long(b3, b2, b1, b0):
    return np.int64(
        (np.uint32(b3) << 24)
        | (np.uint32(b2) << 16)
        | (np.uint32(b1) << 8)
        | np.uint32(b0)
    )


# @njit(inline='always')
def unpack_value(v):
    mult10 = v // 6000
    res = v % 6000
    return res * (10**mult10)
