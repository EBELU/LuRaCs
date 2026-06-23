from pathlib import Path
import numpy as np
from datetime import datetime

from luracs.core import settings
from luracs.containers.spectrum_classes import SpectrumData
from luracs.containers.roi_classes import ROI

class spe_parser:
    def __init__(self, path: Path | str):
        self.data = {}
        
        path = Path(path)

        meas_date = None
        live_time = None
        real_time = None
        calib = None
        data = []
        rois = []

        expected_points = None

        with open(path.resolve(), "r", encoding="utf-8", errors="ignore") as f:
            lines = f.readlines()

        i = 0
        while i < len(lines):
            line = lines[i].strip()

            # ---- DATE ----
            if line.startswith("$DATE_MEA"):
                meas_date = datetime.strptime(lines[i + 1].strip(),
                                            "%m/%d/%Y %H:%M:%S")
                i += 2
                continue

            # ---- MEAS TIME ----
            elif line.startswith("$MEAS_TIM"):
                times = lines[i + 1].split()

                if len(times) == 2:
                    try:
                        live_time, real_time = map(float, times)
                    except ValueError:
                        pass

                i += 2
                continue

            # ---- DATA ----
            elif line.startswith("$DATA"):
                start, end = map(int, lines[i + 1].split())
                expected_points = end - start + 1

                i += 2

                while len(data) < expected_points and i < len(lines):
                    for val in lines[i].split():
                        try:
                            data.append(int(val))
                        except ValueError:
                            pass
                    i += 1

                continue

            # ---- ENERGY ----
            elif line.startswith("$ENER_FIT"):
                calib = list(map(float, lines[i + 1].split()))
                i += 2
                continue

            # ---- ROI ----
            elif line.startswith("$ROI"):
                nr_of_rois = int(lines[i + 1])

                for j in range(nr_of_rois):
                    a, b = map(int, lines[i + 2 + j].split())
                    rois.append((a, b))

                i += 2 + nr_of_rois
                continue

            i += 1
        data = np.array(data)
        self.data = {
            "name": path.name,
            "foreground": SpectrumData(
                data,
                len(data),
                sum(data),
                live_time,
                real_time,
                start_date=meas_date,
                spectrum_name=path.name
            ),
            "calibration": np.array(calib[::-1])
        }
        
class tka_parser:
    def __init__(self, path: Path | str):        
        live_time = None
        real_time = None
        count_array = []
        path = Path(path)
        
        with open(path.resolve(), "r", encoding="utf-8", errors="ignore") as f:
            for i, line in enumerate(f):
                if i > 1:
                    count_array.append(int(line.strip()))
                
                elif i == 0:
                    live_time = int(line.strip())
                    
                elif i == 1:
                    real_time = int(line.strip())
        
        count_array = np.array(count_array)     
        self.data = {
            "name": path.name,
            "foreground": SpectrumData(
                count_array,
                len(count_array),
                sum(count_array),
                live_time,
                real_time,
                spectrum_name=path.name
            )
        }