import json
import logging
import threading
from dataclasses import dataclass
from datetime import datetime

import numpy as np
import requests
from PySide6.QtCore import QObject, Signal


@dataclass(frozen=True)
class SpectrumData:
    y_axis: np.ndarray
    live_time: float
    real_time: float
    dose_rate: float
    count_rate: float
    timestamp: datetime


@dataclass(frozen=True)
class StatusData:
    running: bool
    acquisition_mode: str | None
    location: str | None


class Identifinder400(QObject):
    error_occurred = Signal(str)

    def __init__(self, ip_address: str, parent=None):
        super().__init__(parent)

        self.ip_address = ip_address.rstrip("/")

        if not self.ip_address.startswith(("http://", "https://")):
            self.ip_address = f"http://{self.ip_address}"

        self.log = logging.getLogger("Identifinder400Client")

        self.session = requests.Session()

        self.stream_thread = None
        self.stream_response = None
        self.stream_running = False
        
        
        self.status_data = None
        self.spectrum_data = None

    # ==============================================================
    # HTTP commands
    # ==============================================================

    def command(self, action: str):
        url = f"{self.ip_address}/virtualPha/api/"

        try:
            response = self.session.get(
                url,
                params={"action": action},
                timeout=5,
            )

            response.raise_for_status()

            data = response.json()

            self.log.debug(
                "%s response: %s",
                action,
                data,
            )

            return data

        except requests.RequestException as exc:
            self.error_occurred.emit(
                f"{action}: {exc}"
            )
            return None

        except (ValueError, json.JSONDecodeError) as exc:
            self.error_occurred.emit(
                f"{action}: invalid JSON response: {exc}"
            )
            return None

    def start_acquisition(self):
        return self.command("Start")

    def stop_acquisition(self):
        return self.command("Stop")

    def clear_spectrum(self):
        return self.command("Clear")

    def identify_spectrum(self):
        return self.command("Identify")

    def save_spectrum(self):
        return self.command("Save")


    # ==============================================================
    # SignalR stream
    # ==============================================================

    def start_stream(self):

        if self.stream_running:
            self.log.warning("R400 stream already running")
            return

        self.stream_running = True

        self.stream_thread = threading.Thread(
            target=self._stream_worker,
            name="Identifinder400Stream",
            daemon=True,
        )

        self.stream_thread.start()

        self.log.info(
            "R400 stream thread started: %s",
            self.ip_address,
        )

    def stop_stream(self):
        self.stream_running = False

        thread = self.stream_thread

        if thread is not None:
            self.log.info("Waiting for stream thread to finish")

            # Never wait indefinitely.
            thread.join(timeout=2.0)

            if thread.is_alive():
                self.log.warning(
                    "R400 stream thread did not stop within timeout"
                )
            else:
                self.log.info("R400 stream thread stopped")
                self.stream_thread = None

        self.log.info("R400 stream stopped")

    def _stream_worker(self):

        url = f"{self.ip_address}/virtualPha/signalR"

        self.log.info(
            "Connecting to R400 stream: %s",
            url,
        )

        try:
            response = self.session.get(
                url,
                stream=True,
                timeout=(5, None),
                headers={
                    "Accept": "text/event-stream",
                    "Cache-Control": "no-cache",
                    "Connection": "keep-alive",
                },
            )

            self.stream_response = response

            response.raise_for_status()

            self.log.info(
                "R400 stream connected: HTTP %s",
                response.status_code,
            )

            # Print headers while debugging.
            self.log.debug(
                "R400 stream headers: %s",
                dict(response.headers),
            )

            for line in response.iter_lines(
                decode_unicode=False
            ):

                if not self.stream_running:
                    break

                if not line:
                    continue

                self.log.debug(
                    "R400 raw stream line: %r",
                    line,
                )

                self._process_stream_line(line)

        except requests.RequestException as exc:

            if self.stream_running:
                self.error_occurred.emit(
                    f"R400 stream error: {exc}"
                )

        except Exception as exc:

            self.log.exception(
                "Unexpected R400 stream error"
            )

            if self.stream_running:
                self.error_occurred.emit(
                    f"R400 stream error: {exc}"
                )

        finally:

            self.stream_response = None

            self.log.info(
                "R400 stream worker finished"
            )

    def _process_stream_line(self, line: bytes):
        try:
            text = line.decode("utf-8").strip()

        except UnicodeDecodeError:
            self.log.warning(
                "Invalid UTF-8 from R400: %r",
                line,
            )
            return

        # ----------------------------------------------------------
        # Normal SSE format
        # ----------------------------------------------------------

        if text.startswith("data:"):
            payload = text[5:].strip()

        # ----------------------------------------------------------
        # Some SignalR implementations may provide the JSON
        # directly rather than using "data:".
        # ----------------------------------------------------------

        elif text.startswith("{"):
            payload = text

        else:
            return

        if not payload:
            return

        try:
            message = json.loads(payload)

        except json.JSONDecodeError:

            self.log.debug(
                "Non-JSON R400 stream line: %s",
                payload,
            )

            return

        self.log.debug(
            "R400 decoded message: %s",
            message,
        )

        self._process_message(message)

    # ==============================================================
    # R400 message parsing
    # ==============================================================

    def _process_message(self, message: dict):
        try:
            data = message["M"][0]

        except (KeyError, IndexError, TypeError):

            self.log.debug(
                "Unexpected SignalR message: %s",
                message,
            )

            return

        if not isinstance(data, dict):
            return

        if not data.get("success", False):
            self.log.warning(
                "R400 reported unsuccessful response: %s",
                data,
            )
            return

        misc = data.get("Misc") or {}

        # ----------------------------------------------------------
        # Status
        # ----------------------------------------------------------

        status = StatusData(
            running=bool(
                data.get("running", False)
            ),
            acquisition_mode=misc.get(
                "AcquisitionMode"
            ),
            location=misc.get(
                "Location"
            ),
        )

        self.status_data = status

        # ----------------------------------------------------------
        # Spectrum
        # ----------------------------------------------------------

        spectrum = data.get("Spectrum")

        if spectrum is None:
            return

        spectrum_data = SpectrumData(
            y_axis=np.asarray(
                spectrum,
                dtype=np.int32,
            ),
            live_time=self._parse_seconds(
                misc.get("LiveTime")
            ),
            real_time=self._parse_seconds(
                misc.get("RealTime")
            ),
            dose_rate=self._parse_value(
                misc.get("DoseRate")
            ),
            count_rate=self._parse_value(
                misc.get("GammaRate")
            ),
            timestamp=datetime.now(),
        )


        self.spectrum_data = spectrum_data
        

    # ==============================================================
    # Parsing
    # ==============================================================

    @staticmethod
    def _parse_seconds(value) -> float:

        if value is None:
            return 0.0

        text = str(value).strip()

        # "54.0 s"
        text = text.replace("s", "").strip()

        try:
            return float(text)

        except ValueError:
            return 0.0

    @staticmethod
    def _parse_value(value) -> float:

        if value is None:
            return 0.0

        text = str(value).strip()

        number = ""

        for char in text:

            if char.isdigit() or char in ".-":
                number += char

            elif number:
                break

        try:
            return float(number)

        except ValueError:
            return 0.0

    # ==============================================================
    # Cleanup
    # ==============================================================

    def close(self):

        self.stop_stream()

        try:
            self.session.close()
        except Exception:
            pass



if __name__ == "__main__":
    import sys

    from PySide6.QtCore import QCoreApplication

    logging.basicConfig(
        level=logging.DEBUG,
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    )

    IP_ADDRESS = "http://192.168.154.141"

    app = QCoreApplication(sys.argv)

    client = Identifinder400(IP_ADDRESS)

    # --------------------------------------------------------------
    # Debug callbacks
    # --------------------------------------------------------------

    def on_spectrum(spectrum: SpectrumData):
        print(
            "\n=== SPECTRUM ==="
            f"\nChannels:   {len(spectrum.y_axis)}"
            f"\nLive time:  {spectrum.live_time:.1f} s"
            f"\nReal time:  {spectrum.real_time:.1f} s"
            f"\nDose rate:  {spectrum.dose_rate}"
            f"\nCount rate: {spectrum.count_rate}"
            f"\nTimestamp:  {spectrum.timestamp}"
            f"\nMax count:  {spectrum.y_axis.max()}"
            f"\nTotal:      {spectrum.y_axis.sum()}"
        )

        print(
            "First 20 channels:",
            spectrum.y_axis[:20].tolist(),
        )

    def on_status(status: StatusData):
        print(
            "\n=== STATUS ==="
            f"\nRunning:          {status.running}"
            f"\nAcquisition mode: {status.acquisition_mode}"
            f"\nLocation:         {status.location}"
        )

    def on_error(error: str):
        print(f"\n!!! ERROR: {error}")

    client.spectrum_received.connect(on_spectrum)
    client.status_received.connect(on_status)
    client.error_occurred.connect(on_error)

    # --------------------------------------------------------------
    # Start acquisition + stream
    # --------------------------------------------------------------

    print(f"Connecting to IdentiFinder 400 at {IP_ADDRESS}")

    client.start_stream()
    client.start_acquisition()

    print("Acquisition started.")
    print("Listening for spectrum updates...")
    print("Press Ctrl+C to stop.")

    # --------------------------------------------------------------
    # Keep Qt event loop alive
    # --------------------------------------------------------------

    exit_code = app.exec()

    client.stop_acquisition()
    client.stop_stream()

    sys.exit(exit_code)
