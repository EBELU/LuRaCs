import time


def long_to_bytes(l):
    return bytes([(l >> 24) & 0xFF, (l >> 16) & 0xFF, (l >> 8) & 0xFF, l & 0xFF])


def crc1(data: bytes) -> int:
    CRC = 0
    for k in range(0, len(data), 4):
        rem = len(data) - k
        if rem >= 4:
            CRC += (
                ((data[k + 3] & 0xFF) << 24)
                | ((data[k + 2] & 0xFF) << 16)
                | ((data[k + 1] & 0xFF) << 8)
                | (data[k] & 0xFF)
            )
        elif rem == 3:
            CRC += (
                ((data[k + 2] & 0xFF) << 16)
                | ((data[k + 1] & 0xFF) << 8)
                | (data[k] & 0xFF)
            )
        elif rem == 2:
            CRC += ((data[k + 1] & 0xFF) << 8) | (data[k] & 0xFF)
        else:
            CRC += data[k] & 0xFF
    return CRC


def crc2(data: bytes) -> int:
    out = 0
    for b in data:
        out ^= b
    return out & 0xFF


def calculate_array_crc(data: bytes) -> int:
    crc = 0
    length = len(data)

    for k in range(0, length, 4):
        remaining = length - k

        if remaining >= 4:
            crc += (
                ((data[k + 3] & 0xFF) << 24)
                | ((data[k + 2] & 0xFF) << 16)
                | ((data[k + 1] & 0xFF) << 8)
                | (data[k] & 0xFF)
            )
        elif remaining == 3:
            crc += (
                ((data[k + 2] & 0xFF) << 16)
                | ((data[k + 1] & 0xFF) << 8)
                | (data[k] & 0xFF)
            )
        elif remaining == 2:
            crc += ((data[k + 1] & 0xFF) << 8) | (data[k] & 0xFF)
        else:  # remaining == 1
            crc += data[k] & 0xFF

    return crc


def build_spectrum_request(start_ch=0, end_ch=1799):
    """
    Build a complete spectrum request packet for the detector.

    The packet includes:
    - Command byte (0x3E)
    - Start and end channel
    - CRC1 (32-bit checksum over the payload)
    - CRC2 (8-bit XOR over the inner packet)
    - Header (0xFF) and length byte

    Args:
        start_ch (int, optional): Starting channel index (default 0).
        end_ch (int, optional): Ending channel index (default 1799).

    Returns:
        bytearray: The complete spectrum request packet ready to send over BLE.
    """
    payload = bytearray(5)
    payload[0] = 0x3E
    payload[1] = (start_ch >> 8) & 0xFF
    payload[2] = start_ch & 0xFF
    payload[3] = ((end_ch + 1) >> 8) & 0xFF
    payload[4] = (end_ch + 1) & 0xFF

    crc1_val = crc1(payload)
    crc_bytes = long_to_bytes(crc1_val)
    inner = bytearray([0xEE]) + crc_bytes + payload
    crc2_val = crc2(inner)
    packet = bytearray([0xFF, crc2_val]) + inner + bytearray([len(inner) + 3])
    return packet


def build_tx2_packet(data: bytes) -> bytes:
    """
    Build a TX2 packet exactly like Android startTX2.

    Packet format:
    FF | CRC2 | EE | CRC32(4) | DATA... | LENGTH
    """

    # ---- CRC32-like checksum over payload ----
    crc = calculate_array_crc(data)
    crc_bytes = long_to_bytes(crc)

    # ---- Inner data ----
    # [0xEE][CRC32(4)][payload]
    inner_len = len(data) + 5
    inner = bytearray(inner_len)
    inner[0] = 0xEE
    inner[1:5] = crc_bytes
    inner[5:] = data

    # ---- Final packet ----
    # [0xFF][CRC2][inner][size]
    packet = bytearray(inner_len + 3)
    packet[0] = 0xFF
    packet[1] = crc2(inner)
    packet[2 : 2 + inner_len] = inner
    packet[-1] = inner_len + 3

    return bytes(packet)


def build_ping_packet(active_tab) -> bytes:
    """Builds a package to ping the device, updates which active tab and keeps connection som timing out."""
    now_unix = int(time.time())

    data = bytearray(6)
    data[0] = 0x12  # ping command
    data[1] = active_tab & 0xFF
    data[2] = (now_unix >> 24) & 0xFF
    data[3] = (now_unix >> 16) & 0xFF
    data[4] = (now_unix >> 8) & 0xFF
    data[5] = now_unix & 0xFF

    return bytes(data)
