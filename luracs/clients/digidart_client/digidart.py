"""
digiDART command library via pyusb.

Each command sends exactly the bytes observed in the reference USB trace
and returns the parsed reply as typed Python values. No NIM word lookup,
no dispatch table - just the commands that were captured.

Layout
------
1. Transport constants
2. Reference formatters  (CIBase::Format*, unused by the wire path)
3. DigiDart class:
     lifecycle -> low-level transport -> wire overloads -> commands
   Commands are grouped by subsystem:
     acquisition, timing, high voltage, gain,
     discriminators, stored spectrum, raw spectrum read.
4. Demo block
"""

import logging
import struct

import usb.core
import usb.util

# ============================================================
# Transport constants
# ============================================================

VID = 0x0A2D
PID = 0x0005
EP_OUT = 0x02
EP_IN = 0x82


def csum(body: str) -> int:
    """Sum of ASCII bytes mod 256"""
    return sum(body.encode("ascii")) & 0xFF


def fmt_16(v: int) -> str:
    """Format16 - $C + 5-digit value + 3-digit checksum + CR."""
    body = f"$C{v & 0xFFFF:05d}"
    return f"{body}{csum(body):03d}\r"


def fmt_16_2(a: int, b: int) -> str:
    """Format16 (two values) - $D + 5 + 5 + checksum + CR."""
    body = f"$D{a & 0xFFFF:05d}{b & 0xFFFF:05d}"
    return f"{body}{csum(body):03d}\r"


def fmt_32(v: int) -> str:
    """Format32 - $G + 10-digit value + 3-digit checksum + CR."""
    body = f"$G{v & 0xFFFFFFFF:010d}"
    return f"{body}{csum(body):03d}\r"


def fmt_ascii(s: str) -> str:
    """FormatASCII - $F + string + CR."""
    return f"$F{s}\r"


def fmt_bool(v: int) -> str:
    """FormatBool - $IT or $IF."""
    return "$IT\r" if v else "$IF\r"


# ============================================================
# Transport
# ============================================================


class digiDart:
    """USB transport for the ORTEC digiDART.

    Wire framing (all commands):
        OUT[0..2]   n_cmd       u16 LE
        OUT[2..4]   reply_len   u16 LE
        OUT[4..]    params      u32 BE each, or NUL-terminated strings
        IN[0]       macro error
        IN[1]       micro error
        IN[2..]     command-specific payload
    """

    def __init__(self, dev: usb.Device = None):
        self.log = logging.getLogger("digiDartClient")
        
        if dev is None:
            dev = usb.core.find(idVendor=VID, idProduct=PID)

        if dev is None:
            raise OSError(
                f"digiDART {VID:04x}:{PID:04x} not found"
            )
            
            
        try:
            print("Resetting USB device...")
            dev.reset()
        except usb.core.USBError as e:
            print(f"USB reset failed: {e}")

        self.dev = dev
        self.interface = 0
        self._kernel_driver_detached = False
        self._closed = False

        try:
            try:
                if dev.is_kernel_driver_active(self.interface):
                    dev.detach_kernel_driver(self.interface)
                    self._kernel_driver_detached = True
            except (NotImplementedError, usb.core.USBError):
                pass

            dev.set_configuration()
            usb.util.claim_interface(dev, self.interface)

        except Exception:
            self.close()
            raise

    def close(self):
        if self._closed:
            return

        try:
            usb.util.release_interface(self.dev, 0)
        except usb.core.USBError:
            pass

        try:
            if self._kernel_driver_detached:
                self.dev.attach_kernel_driver(0)
        except (NotImplementedError, usb.core.USBError):
            pass

        self._closed = True

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, traceback):
        self.close()

    # --- executes --- 
    
    def _send_recv(self, cmd: bytes, reply_len: int) -> bytes:
        """Write `cmd`, then read exactly `reply_len` bytes.

        Short replies are padded with zeros so downstream parsing never
        overruns. Raises IOError on USB-level failure.
        """
        self.dev.write(EP_OUT, cmd, timeout=1000)
        try:
            reply = self.dev.read(EP_IN, reply_len, timeout=1000)
        except usb.core.USBError as e:
            raise OSError(f"USB read failed: {e}") from e
        r = bytes(reply)
        if len(r) < reply_len:
            r += b"\x00" * (reply_len - len(r))
        return r

    def _cmd0(self, n_cmd: int) -> tuple[int, int]:
        """Cmd(n_cmd) - 4-byte OUT, 2-byte reply."""
        buf = struct.pack("<HH", n_cmd, 2)
        r = self._send_recv(buf, 2)
        return r[0], r[1]

    def _cmd1(self, n_cmd: int, p0: int) -> tuple[int, int]:
        """Cmd(n_cmd, u32) - 8-byte OUT, 2-byte reply."""
        buf = struct.pack("<HH", n_cmd, 2) + struct.pack(">I", p0 & 0xFFFFFFFF)
        r = self._send_recv(buf, 2)
        return r[0], r[1]

    def _cmd2(self, n_cmd: int, p0: int, p1: int) -> tuple[int, int]:
        """Cmd(n_cmd, u32, u32) - 12-byte OUT, 2-byte reply."""
        buf = struct.pack("<HH", n_cmd, 2) + struct.pack(
            ">II", p0 & 0xFFFFFFFF, p1 & 0xFFFFFFFF
        )
        r = self._send_recv(buf, 2)
        return r[0], r[1]

    # query numbers
    
    def _query_u32_1(self, n_cmd: int, p0: int | None = None):
        """QueryUInt - 1 output, optional 1 input. 6-byte reply."""
        buf = struct.pack("<HH", n_cmd, 6)
        if p0 is not None:
            buf += struct.pack(">I", p0 & 0xFFFFFFFF)
        r = self._send_recv(buf, 6)
        return r[0], r[1], struct.unpack(">I", r[2:6])[0]

    def _query_u32_2(self, n_cmd: int, p0: int | None = None):
        """QueryUInt - 2 outputs, optional 1 input. 10-byte reply."""
        buf = struct.pack("<HH", n_cmd, 10)
        if p0 is not None:
            buf += struct.pack(">I", p0 & 0xFFFFFFFF)
        r = self._send_recv(buf, 10)
        a = struct.unpack(">I", r[2:6])[0]
        b = struct.unpack(">I", r[6:10])[0]
        return r[0], r[1], a, b

    # QueryString

    def _query_string_128(self, n_cmd: int, p0: int | None = None):
        """QueryString - max 128 bytes. 130-byte reply."""
        buf = struct.pack("<HH", n_cmd, 130)
        if p0 is not None:
            buf += struct.pack(">I", p0 & 0xFFFFFFFF)
        r = self._send_recv(buf, 130)
        return r[0], r[1], r[2:]

    # =====================================================================
    # Commands
    # =====================================================================

    # ---- acquisition control ---------------------------------------------

    def start(self) -> tuple[int, int]:
        """START - begin acquisition.  A5 00 | 02 00

        Non-zero micro bits are warnings, not errors:
            0x08 = unit not optimized since init
            0x10 = amplifier not pole-zeroed since init
            0x20 = high voltage disabled
        """
        return self._cmd0(0xA5)

    def stop(self) -> tuple[int, int]:
        """STOP - terminate acquisition.  A9 00 | 02 00"""
        return self._cmd0(0xA9)

    def show_active(self) -> bool:
        """SHOW_ACTIVE - True if acquisition in progress.  5D 00 | 06 00"""
        e3, e4, v = self._query_u32_1(0x5D)
        self._check("SHOW_ACTIVE", e3, e4)
        return v != 0

    def clear(self) -> tuple[int, int]:
        """CLEAR - zero spectrum, live/true counters.
        02 00 | 02 00 | start=0 | count=0x8000"""
        return self._cmd2(0x02, 0, 0x8000)

    def clear_data(self) -> tuple[int, int]:
        """CLEAR_DATA - zero spectrum only.  05 00 | 02 00 | 0 | 0x8000"""
        return self._cmd2(0x05, 0, 0x8000)

    def clear_counters(self) -> tuple[int, int]:
        """CLEAR_COUNTER - zero live/true time counters.  04 00 | 02 00"""
        return self._cmd0(0x04)

    # ---- timing ----------------------------------------------------------

    def show_live(self) -> int:
        """SHOW_LIVE - live time in 20 ms ticks.  7D 00 | 06 00"""
        e3, e4, v = self._query_u32_1(0x7D)
        self._check("SHOW_LIVE", e3, e4)
        return v

    def show_true(self) -> int:
        """SHOW_TRUE - real time in 20 ms ticks.  9A 00 | 06 00"""
        e3, e4, v = self._query_u32_1(0x9A)
        self._check("SHOW_TRUE", e3, e4)
        return v

    # ---- high voltage ----------------------------------------------------

    def enable_hv(self) -> tuple[int, int]:
        """ENABLE_HV - turn on bias supply.  15 00 | 02 00"""
        return self._cmd0(0x15)

    def disable_hv(self) -> tuple[int, int]:
        """DISABLE_HV - turn off bias supply.  0C 00 | 02 00"""
        return self._cmd0(0x0C)

    def set_hv(self, volts: float) -> tuple[int, int]:
        """SET_HV <volts> - the reference truncates to integer.
        3B 00 | 02 00 | <int(volts) BE32>"""
        return self._cmd1(0x3B, int(volts))

    def show_hv_target(self) -> tuple[int, int]:
        """SHOW_HV_TARGET - (target_volts, status_flags).  78 00 | 0A 00

        Status bits:
            bit 0   polarity  (0=positive, 1=negative)
            bit 1   overload  (0=overload, 1=normal)
            bit 2   enabled   (0=disabled, 1=enabled)
        """
        e3, e4, a, b = self._query_u32_2(0x78)
        self._check("SHOW_HV_TARGET", e3, e4)
        return a, b

    def show_hv_actual(self) -> tuple[int, int]:
        """SHOW_HV_ACTUAL - (actual_volts, status_flags).  77 00 | 0A 00"""
        e3, e4, a, b = self._query_u32_2(0x77)
        self._check("SHOW_HV_ACTUAL", e3, e4)
        return a, b

    # ---- gain ------------------------------------------------------------

    def set_gain_coarse(self, gain: int) -> tuple[int, int]:
        """SET_GAIN_COARSE <gain> - 1, 2, 4, 8, 16, or 32.
        34 00 | 02 00 | <gain BE32>"""
        return self._cmd1(0x34, gain)

    def show_gain_coarse(self) -> int:
        """SHOW_GAIN_COARSE.  6F 00 | 06 00"""
        e3, e4, v = self._query_u32_1(0x6F)
        self._check("SHOW_GAIN_COARSE", e3, e4)
        return v

    def set_conversion_gain(self, chans: int) -> tuple[int, int]:
        """SET_GAIN_CONVERSION <chans>.

        Valid values: 512, 1024, 2048, 4096, 8192, 16384. 0 means
        "default" (16384). 35 00 | 02 00 | <chans BE32>
        """
        valid = (512, 1024, 2048, 4096, 8192, 16384)
        if chans != 0 and chans not in valid:
            raise ValueError(
                f"invalid conversion gain {chans}; expected 0 or one of {valid}"
            )
        return self._cmd1(0x35, chans)

    def show_conversion_gain(self) -> int:
        """SHOW_GAIN_CONVERSION - current channel count.  70 00 | 06 00"""
        e3, e4, v = self._query_u32_1(0x70)
        self._check("SHOW_GAIN_CONVERSION", e3, e4)
        return v

    # ---- discriminators --------------------------------------------------

    def set_lld(self, channel: int) -> tuple[int, int]:
        """SET_LLD <channel>.  40 00 | 02 00 | <channel BE32>"""
        return self._cmd1(0x40, channel)

    def show_lld(self) -> int:
        """SHOW_LLD.  81 00 | 06 00"""
        e3, e4, v = self._query_u32_1(0x81)
        self._check("SHOW_LLD", e3, e4)
        return v

    def set_uld(self, channel: int) -> tuple[int, int]:
        """SET_ULD <channel>.  58 00 | 02 00 | <channel BE32>"""
        return self._cmd1(0x58, channel)

    # ---- stored spectrum memory ------------------------------------------

    def show_spectrum_id(self, index: int) -> bytes:
        """SHOW_SPECTRUM_ID <index> - 128-byte ID string.

        FB 00 | 82 00 | <index BE32>

        Returns the raw 128-byte payload (NUL-padded). Raises IOError with
        the manual's description if the device rejects the parameter -
        typically because the stored-spectrum memory is empty.
        """
        e3, e4, s = self._query_string_128(0xFB, index)
        if e3 or e4:
            reason = (
                "invalid first parameter"
                if (e3, e4) == (0x83, 0x80)
                else "device error"
            )
            raise OSError(
                f"SHOW_SPECTRUM_ID error macro={e3:#04x} micro={e4:#04x} ({reason})"
            )
        return s

    def show_spectrum_count(self) -> int:
        """SHOW_SPECTRUM_COUNT - number of stored spectra.  DC 00 | 06 00"""
        e3, e4, v = self._query_u32_1(0xDC)
        self._check("SHOW_SPECTRUM_COUNT", e3, e4)
        return v

    # ---- raw spectrum read -----------------------------------------------

    def spectrum(
        self,
        start: int = 0,
        count: int = 8192,
        chunk_size: int = 0x2000,
    ) -> list[int]:
        """Read `count` channels starting at `start`.

        Per chunk:
            OUT  BB 00 | reply_len LE16 | start BE32 | count BE32
                 reply_len = count * 4 + 4
            IN   [0..4]  4-byte status (0 on success)
                 [4..]   count x u32 BE channel counts

        Chunks at 8192 channels because QueryData rejects larger single
        requests. Returns a flat list of channel counts.
        """
        out: list[int] = []
        chan = start
        remaining = count

        while remaining > 0:
            n = min(remaining, chunk_size)
            reply_len = n * 4 + 4

            buf = (
                struct.pack("<H", 0x00BB)
                + struct.pack("<H", reply_len)
                + struct.pack(">I", chan)
                + struct.pack(">I", n)
            )
            r = self._send_recv(buf, reply_len)

            status = struct.unpack(">I", r[0:4])[0]
            if status != 0:
                raise OSError(f"SPECTRUM status=0x{status:08x} on chunk at {chan}")

            for i in range(n):
                off = 4 + i * 4
                out.append(struct.unpack(">I", r[off : off + 4])[0])

            chan += n
            remaining -= n

        return out

    # ---- internal error check --------------------------------------------

    @staticmethod
    def _check(name: str, e3: int, e4: int) -> None:
        if e3 or e4:
            raise OSError(f"{name} error macro={e3:#04x} micro={e4:#04x}")


# ============================================================
# Demo
# ============================================================

if __name__ == "__main__":
    import time
    with digiDart() as d:

        print("CLEAR                 ->", d.clear())
        print("CLEAR_DATA            ->", d.clear_data())
        print("CLEAR_COUNTER         ->", d.clear_counters())
        print("ENABLE_HV             ->", d.enable_hv())
        print("DISABLE_HV            ->", d.disable_hv())

        print("SET_GAIN_COARSE 4     ->", d.set_gain_coarse(4))
        print("SHOW_GAIN_COARSE      ->", d.show_gain_coarse())
        print("SET_CONVERSION_GAIN   ->", d.set_conversion_gain(8192))
        print("SHOW_CONVERSION_GAIN  ->", d.show_conversion_gain())

        print("SET_HV 0.0            ->", d.set_hv(0.0))
        print("SHOW_HV_TARGET        ->", d.show_hv_target())
        print("SHOW_HV_ACTUAL        ->", d.show_hv_actual())

        print("SET_LLD 1             ->", d.set_lld(1))
        print("SHOW_LLD              ->", d.show_lld())
        print("SET_ULD 8000          ->", d.set_uld(8000))

        print("SHOW_ACTIVE           ->", d.show_active())
        print("SHOW_LIVE             ->", d.show_live())
        print("SHOW_TRUE             ->", d.show_true())

        try:
            print("SHOW_SPECTRUM_ID 1    ->", d.show_spectrum_id(1)[:32].hex())
        except OSError as e:
            print("SHOW_SPECTRUM_ID 1    ->", e)
        print("SHOW_SPECTRUM_COUNT   ->", d.show_spectrum_count())

        print("START                 ->", d.start())
        print("STOP                  ->", d.stop())

        # Full-spectrum read is a separate opcode, not a NIM command.
        channels = d.spectrum(0, 8192)
        print("spectrum              -> total counts =", sum(channels))
        
        time.sleep(1)
        channels = d.spectrum(0, 8192)
        print("spectrum              -> total counts =", sum(channels))

        time.sleep(1)
        channels = d.spectrum(0, 8192)
        print("spectrum              -> total counts =", sum(channels))