from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.utils.file_io import xml_parser

    from .wrapper_types import WrappedSpectrogramData

import numpy as np
from PySide6.QtCore import QObject, Signal

from luracs.containers.roi_classes import SpectrogramROI


class SpectrogramROIHandler(QObject):
    sigRemoveROI = Signal(object)
    sigAddROI = Signal(object)

    def __init__(self, parent):
        super().__init__(parent=parent)

        self.roi_registry: dict[str, SpectrogramROI] = {}
        self.roi_counter: int = 0
        self.roi_edit_mode: bool = False

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
        # Emits the tag of the ROI to be removed when the delete button is clicked
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

    def get_roi_alias_from_tag(self, tag: str):
        return self.roi_registry[tag].alias

    def process_wrapper(
        self, data_packet: WrappedSpectrogramData
    ) -> dict[str, np.ndarray] | None:
        if len(data_packet.wrapped_queues.spectrogram) < 1:
            return

        if self.roi_registry:
            sg = data_packet.wrapped_queues.spectrogram
            Eaxis = data_packet.wrapped_header.x_axis

            results = {}
            for roi in self.roi_registry.values():
                Emin, Emax = roi.E_region
                i0, i1 = np.searchsorted(Eaxis, Emin), np.searchsorted(Eaxis, Emax)
                roi_counts = np.sum(sg[:, i0:i1], axis=1)

                results[roi.tag] = roi_counts / data_packet.wrapped_header.save_interval

            return results
