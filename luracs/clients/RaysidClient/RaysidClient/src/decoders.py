import numpy as np
# from numba import njit, types

from .helpers import (
    two_bytes_to_int,
    three_bytes_to_int,
    four_bytes_to_long,
    unpack_value,
)


def checksum_decode(data: np.array) -> np.uint32:
    out = np.uint32(0)
    length = data.shape[0]
    i = 0
    while i < length:
        b0 = np.uint32(data[i]) if i < length else np.uint32(0)
        b1 = np.uint32(data[i + 1]) if i + 1 < length else np.uint32(0)
        b2 = np.uint32(data[i + 2]) if i + 2 < length else np.uint32(0)
        value = (b0 << 16) | (b1 << 8) | b2
        out ^= value
        i += 3
    return out & np.uint32(0xFFFFFF)


# @njit(types.bool(types.uint16), cache=True, inline = "always")
def check_X(X):
    if X >= 1800 or X < 0:
        return 0
    else:
        return 1


# @njit(types.bool(types.int16, types.int16, types.int8),cache=True, inline="always")
def check_pos(data_len, pos, max_incr):
    if pos + max_incr < data_len:
        return True
    else:
        return False


# @njit(boundscheck=True)
def decode_spectrum_packet(data: np.array, spectrum: np.array):
    """
    Decodes spectrum packets

    Tuple structure
    ERR         = 0
    START_CH    = 1
    END_CH      = 2
    SPECTRUM    = 3
    """

    total_bytes = np.uint16(data[0])
    if total_bytes == 0:
        total_bytes = 256

    if len(data) > total_bytes + 1:
        data = data[: total_bytes + 1]

    if len(data) < 7:
        return 1, 0, 0, spectrum

    data_len = np.int16(len(data))

    if data[1] == 0x30:
        channel_div = 1
    elif data[1] == 0x31:
        channel_div = 3
    elif data[1] == 0x32:
        channel_div = 9
    else:
        return 1, 0, 0, spectrum

    X = two_bytes_to_int(data[2], data[3])
    start_X = two_bytes_to_int(data[2], data[3])

    current_value = three_bytes_to_int(data[4], data[5], data[6])

    for i in range(channel_div):
        if not check_X(X):
            return 2, 0, 0, spectrum
        spectrum[X] = current_value / channel_div
        X += 1

    pos = np.int16(7)

    # Decode loop
    total_new_values = 0
    while pos < (len(data) - 4):
        point_type, point_amount = data[pos] >> 6, data[pos] & 0x3F

        # Special case
        if data[pos] == 0:
            point_type = 4
            point_amount = 1

        pos += 1  # Shift after having read the group info

        # --- Decoding ---
        # 2 4bit values in 1 byte
        if point_type == 0:
            amount_same_type = 0

            while amount_same_type < point_amount:
                if not check_pos(data_len, pos, 1):
                    return 3, 0, 0, spectrum
                diff = np.int32(data[pos] & 0xFF) // 16
                if diff > 7:
                    diff -= 16
                current_value += diff

                for i in range(channel_div):
                    if not check_X(X):
                        return 2, 0, 0, spectrum
                    spectrum[X] = current_value / channel_div
                    X += 1
                total_new_values += 1

                amount_same_type += 1

                if amount_same_type < point_amount:
                    if not check_pos(data_len, pos, 1):
                        return 3, 0, 0, spectrum
                    diff = np.int32(data[pos] & 0xFF) % 16
                    if diff > 7:
                        diff -= 16
                    current_value += diff

                    for i in range(channel_div):
                        if not check_X(X):
                            return 2, 0, 0, spectrum
                        spectrum[X] = current_value / channel_div
                        X += 1
                    total_new_values += 1

                    amount_same_type += 1

                pos += 1

        # 1 8bit value in 1 byte
        elif point_type == 1:
            amount_same_type = 0
            while amount_same_type < point_amount:
                if not check_pos(data_len, pos, 1):
                    return 3, 0, 0, spectrum
                diff = np.int8(data[pos])

                current_value += diff

                total_new_values += 1
                for i in range(channel_div):
                    if not check_X(X):
                        return 2, 0, 0, spectrum
                    spectrum[X] = current_value / channel_div
                    X += 1
                amount_same_type += 1
                pos += 1

        # 2 12bit values in 3 bytes
        elif point_type == 2:
            amount_same_type = 0
            while amount_same_type < point_amount:
                if not check_pos(data_len, pos, 1):
                    return 3, 0, 0, spectrum
                diff = (np.int32(data[pos]) << 4) | (np.int32(data[pos + 1]) >> 4)

                if diff >= 0x800:
                    diff = diff - 0x1000
                else:
                    diff = diff
                current_value += diff
                total_new_values += 1
                for i in range(channel_div):
                    if not check_X(X):
                        return 2, 0, 0, spectrum
                    spectrum[X] = current_value / channel_div

                    X += 1
                amount_same_type += 1
                pos += 2

                if amount_same_type < point_amount:
                    if not check_pos(data_len, pos, 0):
                        return 3, 0, 0, spectrum
                    hi = data[pos - 1].astype(np.int32)
                    lo = data[pos].astype(np.int32)

                    diff = ((hi & 0x0F) << 8) | lo
                    if diff >= 0x800:
                        diff = diff - 0x1000
                    else:
                        diff = diff

                    current_value += diff
                    for i in range(channel_div):
                        if not check_X(X):
                            return 2, 0, 0, spectrum
                        spectrum[X] = current_value / channel_div
                        X += 1
                    amount_same_type += 1
                    pos += 1

        # 1 16bit value in 2 bytes
        elif point_type == 3:
            amount_same_type = 0
            while amount_same_type < point_amount:
                if not check_pos(data_len, pos, 1):
                    return 3, 0, 0, spectrum
                diff = np.int32((data[pos + 1] & 0xFF) << 8) | (data[pos] & 0xFF)
                if diff > 32767:
                    diff -= 65536

                current_value += diff

                total_new_values += 1
                for i in range(channel_div):
                    if not check_X(X):
                        return 2, 0, 0, spectrum
                    spectrum[X] = current_value / channel_div
                    X += 1

                amount_same_type += 1
                pos += 2

        # 1 24bit value in 3 bytes
        elif point_type == 4:
            if not check_pos(data_len, pos, 2):
                return 3, 0, 0, spectrum
            diff = three_bytes_to_int(data[pos + 2], data[pos + 1], data[pos]).astype(
                np.int32
            )
            if diff > 8388607:
                diff -= 16777216

            current_value += diff

            total_new_values += 1
            for i in range(channel_div):
                if not check_X(X):
                    return 2, 0, 0, spectrum
                spectrum[X] = current_value / channel_div
                X += 1

            pos += 3

    return 0, start_X, X, spectrum


# @njit(types.Tuple((types.int64, types.float64, types.float64))(types.uint8[:]), cache=True)
def decode_cps_packet(data) -> tuple:
    """
    Decodes CPS packets

    Tuple structure
    ERR = 0
    CPS = 1
    DR  = 2
    """
    overload = data[14] if len(data) >= 15 else 0
    if overload > 1:
        return 1, 0, 0

    cps = dose = None

    sets = 12 if len(data) > 20 else 2
    for k in range(sets):
        base = k * 3 + 2
        if base + 2 >= len(data):
            break
        dtype = data[base]
        raw = unpack_value(two_bytes_to_int(data[base + 1], data[base + 2]))
        value = raw / 600.0
        if dtype == 0:
            cps = value
        elif dtype == 1:
            dose = value * 10 / 1000
    if cps is not None and dose is not None:
        return 0, cps, dose
    else:
        return 2, 0, 0


def decode_spectrum_meta_packet(data: np.ndarray):
    if len(data) < 17:
        return 1, 0, 0, 0, 0

    spectrum_ticks = four_bytes_to_long(data[5], data[4], data[3], data[2])

    uptime = np.float32(four_bytes_to_long(data[9], data[8], data[7], data[6])) / 12.0

    energy = four_bytes_to_long(data[13], data[12], data[11], data[10])

    highEnergyCounts = three_bytes_to_int(data[14], data[15], data[16])

    if spectrum_ticks < 0 or uptime < 0:
        return 2, 0, 0, 0, 0

    return 0, spectrum_ticks, uptime, energy, highEnergyCounts


# @njit(types.uint16(types.uint8, types.uint8),inline="always")
def u16(lo, hi):
    return np.uint32(hi) << 8 | np.uint32(lo)


# @njit(
#     types.Tuple((
#         types.int64,    # ERR
#         types.int64,    # UART_ERRORS
#         types.int64,    # BATTERY
#         types.float32,  # TEMPERATURE
#         types.boolean,  # TEMP_OK
#         types.boolean,  # CHARGING
#         types.boolean,  # CHANNEL_FULL
#         types.int64,    # CH239
#         types.int64     # AVG_CH239
#     ))(
#         types.uint8[:]
#     ),
#     cache=True
# )
def decode_status_packet(data) -> tuple:
    """
    Decodes status packets.

    Tuple structure
    ERR            = 0
    UART_ERRORS    = 1
    BATTERY        = 2
    TEMPERATURE    = 3
    TEMP_OK        = 4
    CHARGING       = 5
    CHANNEL_FULL   = 6
    CH239          = 7
    AVG_CH239      = 8
    """
    if len(data) < 14:
        return (1, 1, 1, 1, 1, 1, 1, 1, 1)
    temp_raw = u16(data[2], data[3])
    temperature = np.float32(temp_raw / 10.0 - 100.0)
    battery = u16(data[4], data[5]) & 0xFF
    charging = np.bool(data[6] & 0x01)
    ch239 = 700 + u16(data[7], data[8])
    avg_ch239 = 700 + u16(data[9], data[10])
    state = data[11]
    temp_ok = np.bool(state & 0x01)
    channel_full = np.bool(state & 0x02)
    uart_errors = u16(data[12], data[13])

    return (
        0,
        uart_errors,
        battery,
        temperature,
        temp_ok,
        charging,
        channel_full,
        ch239,
        avg_ch239,
    )
