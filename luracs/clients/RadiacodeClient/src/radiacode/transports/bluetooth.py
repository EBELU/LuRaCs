import asyncio
import struct
import time
from dataclasses import dataclass
from typing import Optional

from bleak import BleakClient
from bleak.exc import BleakError
from ..bytes_buffer import BytesBuffer

WRITE_UUID = 'e63215e6-7003-49d8-96b0-b024798fb901'
NOTIFY_UUID = 'e63215e7-7003-49d8-96b0-b024798fb901'


@dataclass
class _Request:
    data: bytes
    future: asyncio.Future


class Bluetooth:
    def __init__(
        self,
        mac: str,
        connect_timeout: float = 5.0,
        max_connect_retries: int = 3,
    ):
        self.mac = mac
        self.connect_timeout = connect_timeout
        self.max_connect_retries = max_connect_retries

        self._client: Optional[BleakClient] = None

        self._queue: asyncio.Queue[_Request] = asyncio.Queue()
        self._task: Optional[asyncio.Task] = None
        self._keepalive_task: Optional[asyncio.Task] = None

        self._running = False

        # response state (owned by BLE worker only)
        self._resp_buffer = bytearray()
        self._resp_remaining = 0
        self._current_future: Optional[asyncio.Future] = None

        # reconnect policy
        self._reconnect_cycles = 0
        self._max_reconnect_cycles = 4

    async def start(self):
        if self._running:
            return

        self._running = True
        self._task = asyncio.create_task(self._worker())

    async def execute(self, req: bytes) -> BytesBuffer:
        if not self._running:
            raise RuntimeError('Bluetooth not started')

        loop = asyncio.get_running_loop()
        fut = loop.create_future()

        await self._queue.put(_Request(req, fut))

        data = await fut
        return BytesBuffer(data)

    async def stop(self):
        self._running = False

        if self._keepalive_task:
            self._keepalive_task.cancel()
            try:
                await self._keepalive_task
            except Exception:
                pass

        if self._task:
            self._task.cancel()
            try:
                await self._task
            except Exception:
                pass

        await self._disconnect()

    async def _worker(self):
        while self._running:
            try:
                await self._ensure_connected()

                req = await self._queue.get()

                try:
                    result = await self._handle_request(req.data)
                    if not req.future.done():
                        req.future.set_result(result)

                except Exception as e:
                    if not req.future.done():
                        req.future.set_exception(e)

            except asyncio.CancelledError:
                break

            except Exception as e:
                # fatal reconnect failure
                self._fail_all_pending(e)
                await self._disconnect()
                break

    async def _ensure_connected(self):
        if self._client and self._client.is_connected:
            return

        await self._disconnect()

        last_err = None

        for attempt in range(1, self.max_connect_retries + 1):
            try:
                self._client = BleakClient(self.mac)

                await asyncio.wait_for(
                    self._client.connect(),
                    timeout=self.connect_timeout,
                )

                if not self._client.is_connected:
                    raise BleakError('not connected')

                await self._client.start_notify(NOTIFY_UUID, self._on_notify)

                self._reconnect_cycles = 0
                return

            except Exception as e:
                last_err = e
                await self._disconnect()

                if attempt < self.max_connect_retries:
                    await asyncio.sleep(0.5 * attempt)

        self._reconnect_cycles += 1

        if self._reconnect_cycles >= self._max_reconnect_cycles:
            raise RuntimeError('Max reconnect cycles exceeded') from last_err

        raise ConnectionError('Connect cycle failed') from last_err

    async def _handle_request(self, req: bytes) -> bytes:
        if not self._client:
            raise ConnectionError('Not connected')

        loop = asyncio.get_running_loop()
        self._current_future = loop.create_future()

        self._resp_buffer = bytearray()
        self._resp_remaining = 0

        for pos in range(0, len(req), 18):
            await self._client.write_gatt_char(
                WRITE_UUID,
                req[pos : pos + 18],
                response=False,
            )

        result = await asyncio.wait_for(self._current_future, timeout=10)

        self._current_future = None
        return result

    def _on_notify(self, _char, data: bytearray):
        if self._resp_remaining == 0:
            self._resp_remaining = 4 + struct.unpack('<i', data[:4])[0]
            self._resp_buffer = bytearray(data[4:])
        else:
            self._resp_buffer.extend(data)

        self._resp_remaining -= len(data)

        if self._resp_remaining == 0 and self._current_future:
            if not self._current_future.done():
                self._current_future.set_result(bytes(self._resp_buffer))

    async def _disconnect(self):
        try:
            if self._client:
                await self._client.disconnect()
        except Exception:
            pass
        finally:
            self._client = None

    def _fail_all_pending(self, error: Exception):
        while not self._queue.empty():
            try:
                req = self._queue.get_nowait()
                if not req.future.done():
                    req.future.set_exception(error)
            except Exception:
                break
