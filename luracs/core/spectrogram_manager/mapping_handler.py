from __future__ import annotations

from datetime import datetime
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .spectrogram_manager import SpectrogramManager
    from .wrapper_types import WrappedSpectrogramData

from collections import namedtuple

import numpy as np
from PySide6.QtCore import QObject

from luracs.utils.file_io import (
    MapFormatParser,
    MapPoint,
    SimpleMappingData,
    export_geojson,
)

MappingData = namedtuple(
    "MappingData", ["longitude", "latitude", "timestamps", "value_dict"]
)

from pathlib import Path


class SimpleMappingBuffer:
    def __init__(
        self,
        longitude: list[float],
        latitude: list[float],
        timestamps: list[float],
        value_dict: dict[str, list[float]],
        meta_data: dict | None = None,
    ):
        self.meta_data = meta_data
        self.longitude = longitude
        self.latitude = latitude
        self.timestamps = timestamps
        self.value_dict = value_dict


class MappingHandler(QObject):
    def __init__(self, spectrogram_manager: SpectrogramManager, parent=None):
        super().__init__(parent=parent)
        self.spectrogram_manager = spectrogram_manager
        self.simple_map_buffer: dict[str, SimpleMappingBuffer] = {}
        self.length_tracker: dict[str, int] = {}

    def process_mapping_data(
        self,
        sg_name: str,
        wrapper_buffer: WrappedSpectrogramData,
        roi_buffer: dict[str, np.ndarray],
    ) -> MappingData | None:
        if sg_name not in self.length_tracker:
            self.length_tracker[sg_name] = 0

        if self.length_tracker[sg_name] == len(
            wrapper_buffer.wrapped_queues.count_rate_queue
        ):
            return

        current_length = self.length_tracker[sg_name]
        
        gps_queue_with_None = wrapper_buffer.wrapped_queues.gps_queue[current_length:]

        is_none = np.frompyfunc(lambda x: x is None, 1, 1)

        mask = ~(
            is_none(gps_queue_with_None).astype(bool)
        )
        
        longitude = [p.longitude for p in gps_queue_with_None[mask]]
        latitude = [p.latitude for p in gps_queue_with_None[mask]]
        timestamps = wrapper_buffer.wrapped_queues.timestamp_queue[current_length:][
            mask
        ]

        value_dict = {
            "count_rate": wrapper_buffer.wrapped_queues.count_rate_queue[
                current_length:
            ][mask],
            "dose_rate": wrapper_buffer.wrapped_queues.dose_rate_queue[current_length:][
                mask
            ],
        }

        for key, item in roi_buffer.items():
            value_dict[key] = item[current_length:][mask]

        self.length_tracker[sg_name] = len(
            wrapper_buffer.wrapped_queues.count_rate_queue
        )

        return MappingData(longitude, latitude, timestamps, value_dict)

    def get_complete_data(
        self, sg_name: str, data_name: str
    ) -> tuple[str, list, list, list, dict[str, list]] | None:
        "Returns the complete mapping data for a given spectrogram and data type (count_rate, dose_rate, or ROI). The returned tuple contains the spectrogram name, lists of longitude, latitude, timestamps, and a dictionary of values corresponding to the specified data type. Explicit returns: (spectrogram_name, longitude_list, latitude_list, timestamps_list, value_dict)"
        if sg_name in self.spectrogram_manager.spectrogram_registry:
            return self.get_complete_data_from_spectrogram(sg_name, data_name)
        else:
            return self.get_complete_data_from_simple_mapping(sg_name, data_name)

    def get_complete_data_from_simple_mapping(
        self, sg_name: str, data_name: str
    ) -> tuple[str, list, list, list, dict[str, list]]:
        buffer = self.simple_map_buffer[sg_name]

        return (
            sg_name,
            buffer.longitude,
            buffer.latitude,
            buffer.timestamps,
            buffer.value_dict,
        )

    def get_complete_data_from_spectrogram(
        self, sg_name: str, data_name: str
    ) -> tuple[str, list, list, list, dict[str, list]]:
        if data_name == "count_rate":
            values = self.spectrogram_manager.get_count_rate_data(sg_name)

        elif data_name == "dose_rate":
            values = self.spectrogram_manager.get_dose_rate_data(sg_name)

        else:
            values = self.spectrogram_manager.get_roi_data(sg_name, data_name)

        wrapper_buffer = self.spectrogram_manager.wrapper_buffer.get(sg_name)
        
        if wrapper_buffer is None or values is None:
            return

        gps_queue_with_None = wrapper_buffer.wrapped_queues.gps_queue

        is_none = np.frompyfunc(lambda x: x is None, 1, 1)

        mask = ~(
            is_none(gps_queue_with_None).astype(bool)
        )
        
        longitude = [p.longitude for p in gps_queue_with_None[mask]]
        latitude = [p.latitude for p in gps_queue_with_None[mask]]

        timestamps = wrapper_buffer.wrapped_queues.timestamp_queue

        is_none = np.frompyfunc(lambda x: x is None, 1, 1)

        # spectrogram_name: str, longitude: list, latitude: list, timestamps: list, value_dict: dict[str, list]
        return (
            sg_name,
            list(longitude),
            list(latitude),
            list(timestamps[mask]),
            {data_name: list(values[mask])},
        )

    def import_simple_mapping_data(self, path: Path) -> str:
        parser = MapFormatParser(path)

        longitude = []
        latitude = []
        timestamps = []
        count_rates = []
        dose_rates = []

        value_dict = {}

        for point in parser.data.data_points:
            longitude.append(point.lng)
            latitude.append(point.lat)
            timestamps.append(point.timestamp.timestamp())
            count_rates.append(point.count_rate)
            dose_rates.append(point.dose_rate)

            if point.other_data_points is not None:
                for key, value in point.other_data_points.items():
                    if key not in value_dict:
                        value_dict[key] = []

                    value_dict[key].append(value)

        value_dict["count_rate"] = count_rates
        value_dict["dose_rate"] = dose_rates

        meta_data = parser.data.meta_data if parser.data.meta_data else {}
        meta_data["device_id"] = parser.data.device_id

        buff = SimpleMappingBuffer(
            longitude=longitude,
            latitude=latitude,
            timestamps=timestamps,
            value_dict=value_dict,
            meta_data=meta_data,
        )

        self.simple_map_buffer[path.name] = buff

        return path.name

    def export_to_geojson(self, sg_name: str, file_path: str | Path):
        "Export the mapped data to a standard GeoJSON GIS file for use in GIS programs like QGis"

        if sg_name in self.spectrogram_manager.spectrogram_registry:
            sg = self.spectrogram_manager.spectrogram_registry[sg_name]

            map_points = []
            for i in range(len(sg.buffers.queues.timestamp_queue)):
                odp = {}
                for roi in self.spectrogram_manager.ROIHandler.roi_registry.values():
                    odp[roi.alias] = self.spectrogram_manager.roi_buffer[sg_name][
                        roi.tag
                    ][i]

                mp = MapPoint(
                    timestamp=datetime.fromtimestamp(
                        sg.buffers.queues.timestamp_queue[i]
                    ),
                    count_rate=sg.buffers.queues.count_rate_queue[i],
                    dose_rate=sg.buffers.queues.dose_rate_queue[i],
                    lng=sg.buffers.queues.gps_queue[i].longitude,
                    lat=sg.buffers.queues.gps_queue[i].latitude,
                    other_data_points=odp,
                )
                map_points.append(mp)

            smd = SimpleMappingData(
                title=sg_name,
                start=datetime.fromtimestamp(sg.start_date),
                end=datetime.fromtimestamp(sg.buffers.latest_values.latest_timestamp),
                device_id=sg.device_id,
                data_points=map_points,
                meta_data={},
            )

        elif sg_name in self.simple_map_buffer:
            buff = self.simple_map_buffer[sg_name]

            map_points = []
            for i in range(len(buff.timestamps)):
                mp = MapPoint(
                    timestamp=datetime.fromtimestamp(buff.timestamps[i]),
                    count_rate=buff.value_dict.get(
                        "count_rate", [None] * len(buff.timestamps)
                    )[i],
                    dose_rate=buff.value_dict.get(
                        "dose_rate", [None] * len(buff.timestamps)
                    )[i],
                    lng=buff.longitude[i],
                    lat=buff.latitude[i],
                    other_data_points={
                        key: values[i]
                        for key, values in buff.value_dict.items()
                        if key not in ["count_rate", "dose_rate"]
                    },
                )
                map_points.append(mp)

            smd = SimpleMappingData(
                title=sg_name,
                start=datetime.fromtimestamp(buff.timestamps[0]),
                end=datetime.fromtimestamp(buff.timestamps[-1]),
                device_id=buff.meta_data.get("device_id", "") if buff.meta_data else "",
                data_points=map_points,
                meta_data=buff.meta_data if buff.meta_data else {},
            )

        else:
            raise ValueError(
                f"Spectrogram or mapping data {sg_name} not found in the registry."
            )

        export_geojson(smd, file_path)
