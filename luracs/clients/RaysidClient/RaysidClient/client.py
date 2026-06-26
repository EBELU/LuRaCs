import asyncio
import time
import platform
import threading
import contextlib
import logging
from enum import Enum, auto

import numpy as np

from bleak import BleakClient, BleakCharacteristicNotFoundError, BleakError
from bleak.exc import BleakDBusError

from .src import (
    logger,
    build_ping_packet,
    build_spectrum_request,
    build_tx2_packet,
    decode_cps_packet,
    decode_spectrum_packet,
    decode_status_packet,
    decode_spectrum_meta_packet,
    checksum_decode,
    CurrentValuesPackage,
    SpectrumResult,
    StatusPackage,
    SpectrumAccumulator,
)


SERVICE_UUID = "49535343-fe7d-4ae5-8fa9-9fafd205e455"
RX_UUID = "49535343-1e4d-4bd9-ba61-23c647249616"
TX_UUID = "49535343-8841-43f4-a8d4-ecbe34729bb3"

if platform.system() == "Windows":
    RX_UUID = RX_UUID.upper()
    TX_UUID = TX_UUID.upper()

_connect_lock = asyncio.Lock()  # ensures only one BLE connect/scan at a time


class RaysidClientAsync:
    """
    Async implementation of the Raysid Client. Generally more stable and more responsive compared with the threaded version but async can be hassle.
    """

    class ErrorFlags(Enum):
        CLEAR = auto()
        CONNECTION_FAILED = auto()
        CONNECTION_TIMED_OUT = auto()
        RECONNETING = auto()

    class EnergyRange(Enum):
        LOW = 8
        MID = 4
        HIGH = 2

    def __init__(self, address, logger=None):

        self.logger = logger or logging.getLogger("RaysidClient")

        self._address = address
        try:
            self.name = address.name
        except AttributeError:
            self.name = "Raysid"

        self._client = BleakClient(
            address, timeout=10, disconnected_callback=self._on_disconnect
        )

        self._latest_cps: CurrentValuesPackage | None = None
        self._latest_status: StatusPackage | None = None

        self._spectrum_accumulator = SpectrumAccumulator()

        self._running = False
        self._disconnected = asyncio.Event()
        self._stopped = False

        self.error_flag = self.ErrorFlags.CLEAR

        self._tx_queue: asyncio.Queue[bytes] = asyncio.Queue()
        self._tx_task: asyncio.Task | None = None
        self._rx_queue: asyncio.Queue[bytes] = asyncio.Queue()
        self._rx_task: asyncio.Task | None = None
        self._reconnect_task: asyncio.Task | None = None
        self._ping_task: asyncio.Task | None = None
        self.active_tab = 1

        self._spectrum_buffer: np.array = np.zeros(1800, dtype=np.float32)

        self.buf: bytearray = bytearray()

    # ------------------ PUBLIC API ------------------
    @property
    def LatestRealTimeData(self):
        return self._latest_cps

    @property
    def LatestStatusData(self):
        return self._latest_status

    @property
    def LatestSpectrum(self):
        return self._spectrum_accumulator.snapshot()

    def reset(self, Erange=EnergyRange.MID):
        logger.debug(f"Reset with E_range {Erange}")
        Erange = np.uint8(Erange.value)
        """
        LOW  - switch to spectrum range 25..1000kev
        MID  - switch to spectrum range 30..2000kev
        HIGH - switch to spectrum range 40..3500kev
        """
        if Erange not in [np.uint(2), np.uint8(4), np.uint(8)]:
            raise RuntimeError(
                f"Invalid energy range in reset, recieved range command {Erange}"
            )

        data = bytearray(2)
        data[0] = 0x10  # command - clear spectrum
        data[1] = Erange
        self.send_packet(data)

    def set_active_tab(self, tab):
        self.active_tab = tab
        ping_packet = build_ping_packet(tab)
        self.send_packet(ping_packet)

    # ------------------ CONTEXT MANAGER ------------------
    async def __aenter__(self):
        started_client = await self.start()
        return started_client

    async def start(self):
        try:
            logger.info(f"📶 Connecting to {self._address}")
            success = False
            connection_attempts = 0
            while not success and connection_attempts < 6:
                connection_attempts += 1
                success = await self._connect()
                if not success:
                    logger.warning(
                        f"❌ Connection attempt {connection_attempts}/5 failed! Retrying in 3s..."
                    )
                    await asyncio.sleep(3)
                    if connection_attempts == 5:
                        logger.critical("Connection failed")
                        await self.stop()
                        return

            self._running = True

            self._tx_task = asyncio.create_task(self._tx_loop())
            self._ping_task = asyncio.create_task(self._ping_loop())
            logger.info("✅ Client started successfully")
            return self
        except asyncio.CancelledError:
            logger.info("🛑 Client start cancelled — cleaning up")
            await self.stop()

            raise

    async def __aexit__(self, exc_type, exc, tb):
        await self.stop()

    async def stop(self):
        self._running = False
        possible_tasks = [
            self._ping_task,
            self._tx_task,
            self._rx_task,
            self._reconnect_task,
        ]

        # Cancel tasks
        for t in possible_tasks:
            if t and not t.done():
                t.cancel()

        # Force disconnect to unblock BLE tasks
        if self._client.is_connected:
            try:
                await self._client.stop_notify(RX_UUID)
                await self._client.disconnect()
            except Exception as e:
                logger.warning(f"Disconnect failed: {e}")

        self._stopped = True
        # Wait for tasks to finish
        try:
            await asyncio.wait_for(
                asyncio.gather(
                    *[t for t in possible_tasks if t], return_exceptions=True
                ),
                timeout=3,
            )
        except asyncio.TimeoutError:
            logger.warning("Some internal tasks did not finish in time")

    # ------------------ PRIVATE METHODS ------------------

    def _on_disconnect(self, client):

        if self._running:
            logger.warning(f"❌ BLE device {self.device_name} disconnected!")
            if not self._stopped and (
                self._reconnect_task is None or self._reconnect_task.done()
            ):
                self._reconnect_task = asyncio.create_task(self._reconnect_loop())

        self._running = False
        self._disconnected.set()

    async def _reconnect_loop(self):
        """Attempt to reconnect until successful or client is stopped."""
        try:
            max_attempts = 5  # 0 = infinite
            attempt = 0

            restart_tasks = [
                task
                for task in (self._tx_task, self._rx_task, self._ping_task)
                if task and not task.done()
            ]

            for task in restart_tasks:
                task.cancel()

            for task in restart_tasks:
                with contextlib.suppress(asyncio.CancelledError):
                    await task

            while not self._stopped:
                attempt += 1
                if max_attempts and attempt > max_attempts:
                    logger.error(
                        "❌ Maximum reconnection attempts reached! Stopping client"
                    )
                    self._stopped = True
                    break

                logger.info(
                    f"🔄 Attempting to reconnect to {self.device_name} (try {attempt})..."
                )
                success = await self._connect()
                if success:
                    logger.info(f"✅ Reconnected to {self.device_name}")
                    self._running = True
                    # Restart TX and ping loops

                    self._tx_task = asyncio.create_task(self._tx_loop())
                    self._rx_task = asyncio.create_task(self._packet_worker())
                    self._ping_task = asyncio.create_task(self._ping_loop())
                    self._disconnected.clear()
                    break

                await asyncio.sleep(3)  # wait 3s before retry
        except asyncio.CancelledError:
            logger.info("🛑 Reconnect loop cancelled")
            await self.stop()
            raise

    async def _start_notify(self, uuid, callback):
        try:
            await self._client.start_notify(uuid, callback)
            logger.info(f"✅ Notifications started")
            return True
        except (BleakCharacteristicNotFoundError, BleakError) as e:
            logger.debug(f"❌ Could not start notifications on {uuid}")
            logger.debug(f"{str(e)}")
            return False

    async def _connect(self):
        """Connect safely, retrying if BlueZ is busy"""
        async with _connect_lock:
            try:
                await self._client.connect()
                await asyncio.sleep(0.5)

                success = await self._start_notify(RX_UUID, self._handle_notification)
                if not success:
                    await self._client.disconnect()
                    return False

                self._rx_queue = asyncio.Queue(maxsize=256)
                self._rx_task = asyncio.create_task(self._packet_worker())

                if isinstance(self._address, str):
                    self.device_name = self._address
                else:
                    self.device_name = self._address.name

                logger.info(f"✅ Connected to {self.device_name}")
                return True
            except asyncio.TimeoutError:
                logger.warning("⏱️ Connection timeout")
                return False

            except BleakDBusError as e:
                if "InProgress" in str(e):
                    logger.warning(f"⚠️ BLE busy")
                    return False
                else:
                    return False

            except BleakError as e:
                logger.debug(f"❌ Bleak error during connect: {e}")
                return False

            except Exception as e:
                logger.error(f"❌ Unexpected connect error: {e}")
                return False

    async def _tx_loop(self):
        """
        Single-writer BLE transmit loop.
        """
        while self._running and self._client.is_connected:
            try:
                packet = await self._tx_queue.get()

                packet = build_tx2_packet(packet)
                await self._client.write_gatt_char(TX_UUID, packet, response=True)
                await asyncio.sleep(0.5)

            except asyncio.CancelledError:
                break
            except Exception as e:
                logger.error(f"❌ TX failed: {e}")

    def send_packet(self, packet: bytes):
        """Put a byte packet in to tx_queue. No need to await."""
        self._tx_queue.put_nowait(packet)

    async def _ping_loop(self):
        """Pings the device every 10s to maintain connection"""
        try:
            while self._running and self._client.is_connected:
                ping_packet = build_ping_packet(self.active_tab)
                self.send_packet(ping_packet)
                logger.debug(f"Ping! {self.active_tab}")
                await asyncio.sleep(10)

        except asyncio.CancelledError:
            pass

    def _handle_notification(self, _, data: bytearray):
        """
        Just copy bytes and enqueue.
        """
        try:
            self._rx_queue.put_nowait(bytes(data))
        except asyncio.QueueFull:
            logger.warning("⚠️ RX queue full — dropping packet")

    async def _packet_worker(self):
        """
        Background task that processes BLE packets.
        """
        try:
            while self._running and self._rx_task:
                data = await self._rx_queue.get()

                try:
                    self._process_packet(data)
                except Exception:
                    logger.exception("Packet processing failed")
        except asyncio.CancelledError:
            # cleanly exit loop
            return

    def _process_packet(self, data: bytes):

        # Dispatch table
        handlers = {
            0x01: self._handle_meta_packet,
            0x02: self._handle_status_packet,
            0x17: self._handle_cps_packet,
            0x30: self._handle_spectrum_packet,
            0x31: self._handle_spectrum_packet,
            0x32: self._handle_spectrum_packet,
        }

        # Logic
        if len(data) < 2:
            return

        self.buf.extend(data)

        # Check expected length of spectrum packet
        expected_len = int(self.buf[0]) or 256

        # Packets can arrive so there are several packets in the buffer at the same time
        while expected_len <= len(self.buf):
            packet = bytes(self.buf[: expected_len + 1])
            pkt_type = packet[1]
            del self.buf[: expected_len + 1]

            buf = np.frombuffer(packet, dtype=np.uint8)

            if pkt_type in handlers:
                handlers[pkt_type](buf)

            elif pkt_type in (0x03, 0x06):
                # Dont know what these are
                logger.debug(f"0x{pkt_type:02X} packet")

            else:
                logger.debug(
                    f"Unknown packet type {pkt_type} containing {bytes(packet).hex()}"
                )
                self.buf.clear()  # Panic clean buffer to avoid blocking
                return

            if len(self.buf):
                expected_len = int(self.buf[0]) or 256
            else:
                break

    def _handle_cps_packet(self, data: bytes):
        if len(data) < 6:
            return

        payload = data[1:-3]

        calculated = checksum_decode(payload)

        calc_byte0 = (calculated >> 16) & 0xFF  # High
        calc_byte1 = (calculated >> 8) & 0xFF  # Mid
        # calc_byte2  = calculated & 0xFF       # Low, Unused

        # Convert stored little-endian checksum to integer
        exp_byte1 = data[-4]
        exp_byte0 = data[-3]

        # Two byte comparison
        if calc_byte0 != exp_byte0 or calc_byte1 != exp_byte1:
            logger.debug(
                f"Invalid CPS checksum: "
                f"expected=[{exp_byte0:02X},{exp_byte1:02X}] "
                f"calculated=[{calc_byte0:02X},{calc_byte1:02X}]"
            )
            return

        cps = decode_cps_packet(data)

        ERR, CPS, DR = range(3)

        error_code = cps[ERR]
        if error_code:
            if error_code == 1:
                logger.debug("⚠️ OVERLOAD — CPS not updated")
            elif error_code == 2:
                logger.debug("⚠️ No CPS returned, unknown error")
            else:
                logger.debug("CPS decode failed")

        if cps[CPS] > 0 and cps[DR] > 0:
            self._latest_cps = CurrentValuesPackage(cps[CPS], cps[DR], time.time())

    def _handle_status_packet(self, data: bytes):
        status = decode_status_packet(data)

        (
            ERR,
            UART_ERRORS,
            BATTERY,
            TEMPERATURE,
            TEMP_OK,
            CHARGING,
            CHANNEL_FULL,
            CH239,
            AVG_CH239,
        ) = range(9)

        if status[ERR] != 0:
            logger.debug("Status decode failed")

        if status[UART_ERRORS] == 0:
            self._latest_status = StatusPackage(*status[BATTERY:], time.time())

    def _handle_spectrum_packet(self, data: np.array):
        """
        Process a spectrum packet

        Packet structure [len][type][...data...][chk1][chk2][chk3]
        """

        if len(data) < 6:
            return
        if len(data) > 256:  # Separator or someting
            data = data[:256]
        # Checksum is calculated over whole packet
        for attempt in range(2):  # Try the checksum twice
            payload = data[:-3]

            calculated = checksum_decode(payload)

            # Convert stored little-endian checksum to integer
            checksum_bytes = data[-3:]
            expected = (
                np.uint32(checksum_bytes[0])
                | (np.uint32(checksum_bytes[1]) << 8)
                | (np.uint32(checksum_bytes[2]) << 16)
            )

            if calculated == expected:
                # All good
                break
            elif calculated != expected and attempt == 0:
                # Try again!
                data = data[:-1]
            else:
                # Ok i guess it was wrongf
                logger.debug(
                    f"Invalid Spectrum checksum: expected=0x{expected:06X} calculated=0x{calculated:06X}"
                )
                return

        self._spectrum_buffer.fill(0)
        spectrum_result = decode_spectrum_packet(data, self._spectrum_buffer)

        error_code = spectrum_result[0]
        if error_code == 0:
            self._spectrum_accumulator.insert(*spectrum_result[1:])
        elif error_code == 1:
            logger.debug("Invalid spectrum packet")
        elif error_code == 2:
            logger.debug("Index error in X")
        elif error_code == 3:
            logger.debug("Index error in pos")

    def _handle_meta_packet(self, data: bytes):
        result = decode_spectrum_meta_packet(data)

        if result[0] == 0:
            self._spectrum_accumulator.update_meta(*result[1:])
            return
        elif result[0] == 1:
            logger.debug("Invalid meta packet length")
        elif result[0] == 2:
            logger.debug("Overflow in meta packet")


import threading
import traceback


class RaysidClient:
    """
    Threaded wrapper around RaysidClientAsync.

    Provides synchronous API access from another thread.
    """

    def __init__(self, address):
        self._address = address
        self._loop: asyncio.AbstractEventLoop | None = None
        self._thread: threading.Thread | None = None
        self._client: RaysidClientAsync | None = None

        self._started = threading.Event()
        self._stopped = threading.Event()
        self.on_disconnect = None

    # -------------------- Public API --------------------
    def reset(self, Erange: int = 2):
        if not self._client or not self._loop:
            raise RuntimeError("Client not started")

        done = threading.Event()
        error = []

        def wrapper():
            try:
                self._client.reset(Erange)
            except Exception as e:
                error.append(e)
            finally:
                done.set()

        self._loop.call_soon_threadsafe(wrapper)

        if not done.wait(timeout=5):
            raise TimeoutError("Reset timed out")

        if error:
            raise error[0]

    def set_active_tab(self, tab: int):
        if not self._client or not self._loop:
            raise RuntimeError("Client not started")

        done = threading.Event()
        error = []

        def wrapper():
            try:
                self._client.set_active_tab(tab)
            except Exception as e:
                error.append(e)
            finally:
                done.set()

        self._loop.call_soon_threadsafe(wrapper)

        if not done.wait(timeout=5):
            raise TimeoutError("set_active_tab timed out")

        if error:
            raise error[0]

    @property
    def LatestRealTimeData(self):
        return self._client.LatestRealTimeData if self._client else None

    @property
    def LatestStatusData(self):
        return self._client.LatestStatusData if self._client else None

    @property
    def LatestSpectrum(self):
        return self._client.LatestSpectrum if self._client else None

    # -------------------- Context Manager --------------------
    def __enter__(self):
        self.start()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.stop()

    # -------------------- Lifecycle --------------------
    def start(self):
        if self._thread and self._thread.is_alive():
            return

        self._thread = threading.Thread(target=self._run_loop, daemon=True)
        self._thread.start()

        if not self._started.wait(timeout=10):
            raise RuntimeError("RaysidClient failed to start")

    def stop(self):
        if not self._loop or self._stopped.is_set():
            return

        async def shutdown():
            try:
                if self._client:
                    await self._client.stop()
            except Exception:
                traceback.print_exc()
            finally:
                self._loop.stop()

        try:
            fut = asyncio.run_coroutine_threadsafe(shutdown(), self._loop)
            fut.result(timeout=5)
        except Exception:
            traceback.print_exc()

        if self._thread:
            self._thread.join(timeout=5)
            self._thread = None

        self._loop = None
        self._client = None
        self._stopped.set()

    # -------------------- Internal --------------------
    def _run_loop(self):
        self._loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self._loop)

        main_task = self._loop.create_task(self._start_client())

        try:
            self._loop.run_forever()
        finally:
            for task in asyncio.all_tasks(self._loop):
                task.cancel()

            self._loop.run_until_complete(
                asyncio.gather(*asyncio.all_tasks(self._loop), return_exceptions=True)
            )
            self._loop.close()
            self._stopped.set()

    async def _start_client(self):
        self._client = RaysidClientAsync(self._address)
        # Wrap disconnect callback
        self._client._parent_on_disconnect = self._handle_disconnect
        await self._client.start()
        self._started.set()

    def _handle_disconnect(self):
        if self.on_disconnect:
            self.on_disconnect()

    def __del__(self):
        # Force stop in case of improper shutdown
        try:
            self.stop()
        except Exception:
            pass


if __name__ == "__main__":
    with RaysidClient("60:8A:10:32:15:43") as client:
        for i in range(40):
            print(client.LatestRealTimeData)

            time.sleep(0.5)
