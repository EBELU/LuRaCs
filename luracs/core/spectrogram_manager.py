from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.spectrogram import Spectrogram, WrappedSpectrogramData
    from luracs.utils.file_io import xml_parser

    from .run_manager import _RunManager


import numpy as np
from PySide6.QtCore import QObject, Signal

from luracs.containers.roi_classes import SpectrogramROI


class SpectrogramManager(QObject):
    sigMapBufferUpdated = Signal()
    sigSpectrogramBufferUpdated = Signal()

    sigAddROI = Signal(object)
    sigRemoveROI = Signal(object)

    sigROICountsUpdated = Signal(str, object)

    def __init__(self, run_manager: _RunManager, parent=None):
        super().__init__(parent=parent)
        self.run_manager = run_manager

        self.spectrogram_registry: dict[str, Spectrogram] = {}
        self.roi_registry: dict[str, SpectrogramROI] = {}
        self.energy_axes_buffer: dict[str, np.ndarray] = {}
        self.roi_counter = 0
        self.roi_edit_mode = False

    def set_roi_edit_mode(self, state: bool):
        self.roi_edit_mode = state

    def add_roi(self, lower: float, upper: float, **kwargs):
        tag = f"SG_ROI_{self.roi_counter}"
        self.roi_counter += 1

        new_roi = SpectrogramROI(
            tag=tag,
            E_region=np.array([lower, upper]),
            alias=kwargs.get("alias", tag),
            movable=self.roi_edit_mode,
            emission=kwargs.get("emission"),
        )

        self.roi_registry[tag] = new_roi
        new_roi.sigDeleteRequested.connect(self.remove_roi)
        self.sigAddROI.emit(new_roi)

    def remove_roi(self, tag: str):
        popped_roi = self.roi_registry.pop(tag, None)

        if popped_roi is None:
            return

        self.sigRemoveROI.emit(popped_roi)

    def clear_rois(self):
        for tag in self.roi_registry.copy():
            self.remove_roi(tag)

    def load_rois(self, parser: xml_parser):
        rois = parser.get_rois()
        for peak in rois:
            extended_kwargs = {
                **peak.__dict__,
                **peak.meta,
            }
            del extended_kwargs["tag"]
            self.add_roi(
                *peak.roi_bound,
                alias=extended_kwargs["alias"],
                emission=extended_kwargs["emission"],
            )

    def receive_buffer(self, logger_name: str, buffer: WrappedSpectrogramData):
        if not self.roi_registry or len(buffer.spectrogram) < 1:
            return

        sg = np.vstack(buffer.spectrogram)

        Eaxis = self.energy_axes_buffer[logger_name]
        results = {}
        for roi in self.roi_registry.values():
            Emin, Emax = roi.E_region
            roi_counts = np.sum(sg[:, ((Emin < Eaxis) & (Eaxis < Emax))], axis=1)

            results[roi.tag] = roi_counts / buffer.save_interval

        self.sigROICountsUpdated.emit(logger_name, results)
