from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .wrapper_types import WrappedSpectrogramData

import logging

gui_logger = logging.getLogger("Application")

import numpy as np
from PySide6.QtCore import QObject, Signal, Slot

from ..run_manager import RunManager
from ..spectrum_manager import SpectrumManager
from .mapping_handler import MappingHandler
from .roi_handler import SpectrogramROIHandler
from .spectrogram import Spectrogram


class SpectrogramManager(QObject):
    sigDataUpdated = Signal(str, object)
    sigMappingDataUpdated = Signal(str, list, list, list, dict)
    sigROIDataUpdated = Signal(str, dict)
    sigSpectrogramStarted = Signal(str)
    sigSpectrogramClosed = Signal(str)
    sigSpectrogramDequeResized = Signal()

    def __init__(self):
        super().__init__(parent=None)

        self.spectrogram_registry: dict[str, Spectrogram] = {}
        self.wrapper_buffer: dict[str, WrappedSpectrogramData] = {}
        self.roi_buffer: dict[str, dict] = {}

        self.ROIHandler = SpectrogramROIHandler(self)
        self.MappingHandler = MappingHandler(self)

    def restart_spectrogram(self, db_name: str):
        "Loads an existing spectrogram based on the file name. The file must be stored in the data library folder before loading."
        new_log = Spectrogram(db_name, resume=True)

        # Connect the signals from the RunManager to the new spectrogram instance
        RunManager.Signals.currentUpdated.connect(new_log.receive_current)
        RunManager.Signals.statusUpdated.connect(new_log.receive_status)
        RunManager.Signals.spectrumUpdated.connect(new_log.receive_spectrum)
        RunManager.Signals.GPSUpdated.connect(new_log.receive_gps)

        # Connect the data updated signal to the handler, other components should connect to the SpectrogramManager's sigDataUpdated signal instead of the individual spectrogram's signal
        new_log.sigDataUpdated.connect(self._on_spectrogram_update)

        self.spectrogram_registry[db_name] = new_log
        gui_logger.info(f"Spectrogram {db_name} resumed")
        new_log.request_data()
        self.sigSpectrogramStarted.emit(db_name)

    def start_new_spectrogram(
        self, db_name: str, device: str, save_interval: int = 1, concat: int = 0
    ):
        "Starts a new spectrogram based on the device name. The device must be registered in the device registry."
        device_wrapper = RunManager.device_registry.get(device, None)
        if not device_wrapper:
            gui_logger.warning(
                f"Logging could not be started as {device} does not exist"
            )
            return

        calibration_coeff = None
        for spectrum in SpectrumManager.get_spectra_dict().values():
            if device == spectrum.connection:
                calibration_coeff = spectrum.calibration_coefficients
                break

        db_name = db_name.replace(":", "-")  # Replace forbidden characters on windows
        new_log = Spectrogram(
            db_name,
            save_interval=save_interval,
            spect_channels=device_wrapper.channels,
            device_id=spectrum.instrument.name
            if spectrum.instrument
            else device_wrapper.name,
            calibration_coeff=calibration_coeff
            if calibration_coeff is not None
            else [],
            channel_concat_factor=concat,
            header_meta={"version": 1, "gps_version": 1},
        )

        RunManager.Signals.currentUpdated.connect(new_log.receive_current)
        RunManager.Signals.statusUpdated.connect(new_log.receive_status)
        RunManager.Signals.spectrumUpdated.connect(new_log.receive_spectrum)
        RunManager.Signals.GPSUpdated.connect(new_log.receive_gps)

        new_log.sigDataUpdated.connect(self._on_spectrogram_update)

        self.spectrogram_registry[db_name] = new_log
        gui_logger.info(f"Spectrogram {db_name} started")
        self.sigSpectrogramStarted.emit(db_name)
        new_log.request_data()

    def close_spectrogram(self, db_name: str):
        "Closes a spectrogram and removes it from the registry. The SQLite database file will remain in the data library folder."
        # Fetch the spectrogram from the registry and close it
        old_sg = self.spectrogram_registry.pop(db_name, None)
        if old_sg is not None:
            old_sg.close()  # Close the SQLite connection
            old_sg.deleteLater()  # Delete the QObject to free resources
        else:
            raise ValueError(f"Spectrogram {db_name} not found in the registry.")

        self.sigSpectrogramClosed.emit(db_name)
        gui_logger.info(f"Spectrogram {db_name} closed")

        # Remove the associated data from the buffers
        self.wrapper_buffer.pop(db_name, None)
        self.roi_buffer.pop(db_name, None)

    @Slot()
    def _on_spectrogram_update(
        self, sg_name: str, data_wrapper: WrappedSpectrogramData
    ):
        "Slot to handle the data updated signal from a spectrogram. It stores the updated data wrapper in the buffer and emits the signal to notify listeners."
        # Store the updated data wrapper in the buffer and emit the signal to notify listeners
        self.wrapper_buffer[sg_name] = data_wrapper
        self.sigDataUpdated.emit(sg_name, data_wrapper)

        # Process the ROI data using the ROIHandler and emit the signal if ROI data is available
        rois = self.ROIHandler.process_wrapper(data_wrapper)
        if rois is not None:
            self.roi_buffer[sg_name] = rois
            self.sigROIDataUpdated.emit(sg_name, rois)

        if any(data_wrapper.wrapped_queues.gps_queue):
            processed_mapping_data = self.MappingHandler.process_mapping_data(
                sg_name, data_wrapper, rois if rois is not None else {}
            )
            if processed_mapping_data is not None:
                self.sigMappingDataUpdated.emit(sg_name, *processed_mapping_data)

    # --- Getters ---
    def get_roi_data(self, sg_name: str, roi_tag: str) -> np.ndarray | None:
        return self.roi_buffer.get(sg_name, {}).get(roi_tag, None)

    def get_count_rate_data(self, sg_name: str) -> np.ndarray | None:
        return (
            self.wrapper_buffer.get(sg_name, None).wrapped_queues.count_rate_queue
            if self.wrapper_buffer.get(sg_name, None)
            else None
        )

    def get_dose_rate_data(self, sg_name: str) -> np.ndarray | None:
        return (
            self.wrapper_buffer.get(sg_name, None).wrapped_queues.dose_rate_queue
            if self.wrapper_buffer.get(sg_name, None)
            else None
        )

    def get_gps_data(self, sg_name: str) -> np.ndarray | None:
        return (
            self.wrapper_buffer.get(sg_name, None).wrapped_queues.gps_queue
            if self.wrapper_buffer.get(sg_name, None)
            else None
        )

    def get_timestamps(self, sg_name: str) -> np.ndarray | None:
        return (
            self.wrapper_buffer.get(sg_name, None).wrapped_queues.timestamp_queue
            if self.wrapper_buffer.get(sg_name, None)
            else None
        )
