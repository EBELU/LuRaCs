import json
from dataclasses import dataclass, asdict
from datetime import datetime
from pathlib import Path


@dataclass(frozen=True)
class MapPoint:
    timestamp: datetime
    count_rate: float
    dose_rate: float | None
    lng: float
    lat: float
    other_data_point: dict[float] | None = None


@dataclass
class SimpleMappingData:
    title: str
    device_id: str
    start: datetime
    end: datetime | None
    data_points: list[MapPoint]
    meta_data: dict


class MapFormatParser:
    def __init__(self, file_path: Path | str):
        file_path = Path(file_path)

        if file_path.suffix == ".rctrk":
            self._parsed_data = self.parse_radiacode_track(file_path)
        elif file_path.suffix == ".geojson":
            self._parsed_data = self.parse_geojson(file_path)

    @property
    def data(self) -> SimpleMappingData:
        return self._parsed_data

    def parse_radiacode_track(self, file_path: Path) -> SimpleMappingData:
        with open(file_path) as f:
            json_data = json.load(f)

        is_Sv = json_data["sv"]

        start_time = datetime.fromtimestamp(json_data["periods"][0]["start"])
        end_time = datetime.fromtimestamp(json_data["periods"][-1]["end"])

        sievert_per_rontgen = 0.0088

        points = [
            MapPoint(
                timestamp=datetime.fromtimestamp(marker["date"]),
                count_rate=marker["countRate"],
                dose_rate=marker["doseRate"]
                if is_Sv
                else marker["doseRate"] * sievert_per_rontgen,
                lng=marker["lon"],
                lat=marker["lat"],
            )
            for marker in json_data["markers"]
        ]

        device_id = json_data["devices"][0]

        title = json_data["title"]

        return SimpleMappingData(file_path.name, device_id, start_time, end_time, points, {"title": title})
    
    def parse_geojson(self, file_path: str) -> SimpleMappingData:
        with open(file_path, "r", encoding="utf-8") as f:
            geojson = json.load(f)

        metadata = geojson.get("metadata", {})

        data_points = []

        for feature in geojson["features"]:
            lon, lat = feature["geometry"]["coordinates"]
            props = feature["properties"]

            data_points.append(
                MapPoint(
                    timestamp=datetime.fromisoformat(props["timestamp"]),
                    count_rate=props["count_rate"],
                    dose_rate=props.get("dose_rate"),
                    lng=lon,
                    lat=lat,
                    other_data_point=props.get("other_data_point"),
                )
            )

        return SimpleMappingData(
            title=file_path.name,
            device_id=metadata.get("device_id", ""),
            start=datetime.fromisoformat(metadata["start"]),
            end=(
                datetime.fromisoformat(metadata["end"])
                if metadata.get("end") is not None
                else None
            ),
            data_points=data_points,
            meta_data={
                k: v
                for k, v in metadata.items()
                if k not in {"device_id", "start", "end"}
            },
        )
    
    
def export_geojson(mapping_data: SimpleMappingData, file_path: str | Path):
    "Export the mapped data to a standard GeoJSON GIS file for use in GIS programs like QGis"
    file_path = Path(file_path)
    features = []

    for point in mapping_data.data_points:
        properties = {
            "timestamp": point.timestamp.isoformat(),
            "count_rate": point.count_rate,
            "dose_rate": point.dose_rate,
            "other_data_point": point.other_data_point,
        }

        features.append({
            "type": "Feature",
            "geometry": {
                "type": "Point",
                "coordinates": [point.lng, point.lat],  # GeoJSON is [lon, lat]
            },
            "properties": properties,
        })

    geojson = {
        "type": "FeatureCollection",
        "name": mapping_data.title,
        "metadata": {
            "device_id": mapping_data.device_id,
            "start": mapping_data.start.isoformat(),
            "end": mapping_data.end.isoformat() if mapping_data.end else None,
            **mapping_data.meta_data,
        },
        "features": features,
    }

    with open(file_path.with_suffix(".geojson"), "w", encoding="utf-8") as f:
        json.dump(geojson, f, indent=2)
    
if __name__ == "__main__":
    pth = Path("~/NS2 - pinne och kullen.rctrk")
    parser = MapFormatParser(pth)
    
    print(parser.data)
