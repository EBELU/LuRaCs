from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pytestqt import qtbot as qtbot_t

    from luracs.main import MainWindow
    
from datetime import datetime

import numpy as np
import pytest

from luracs.core import IOManager, RunManager
from luracs.utils import file_io

pytestmark = pytest.mark.order(2)

def test_spectrogram_start(script_engine, qtbot):
    script_engine.submit_from_sync("spectrogram start all")
    qtbot.wait(2000)
    
    assert len(RunManager.SpectrogramManager.spectrogram_registry)
    
def test_spectrogram_time_selector(main_window: MainWindow, qtbot: qtbot_t):
    main_window.spectrogram.action_time_selector.trigger()
    qtbot.wait(500)
    main_window.spectrogram.action_time_selector.trigger()
    
def test_import_spectrogram(main_window: MainWindow, qtbot: qtbot_t, test_data):
    IOManager.Importer.import_spectrograms(test_data / "Debug_Sg.db")
    qtbot.wait(500)

    main_window.spectrogram.spectrogram_selection.setCurrentIndex(0)
    
    qtbot.wait(500)
    
    main_window.spectrogram.spectrogram_selection.setCurrentIndex(1)
    
def test_spectrogram_to_spectrum(main_window: MainWindow, qtbot: qtbot_t, tmp_path):
    main_window.spectrogram.action_time_selector.setChecked(True)
    assert main_window.spectrogram.info_text_selector != ""
    
    current_spectrogram_name = main_window.spectrogram.spectrogram_selection.currentData()
    
    ymin, ymax = main_window.spectrogram.time_selector.getRegion()
    ymin, ymax = round(ymin), round(ymax)

    timestamps = np.array(main_window.spectrogram.current_packet_buffer.timestamp_deque)[::-1]

    start_index = max(0, min(ymax - 1, len(timestamps) - 1))
    stop_index = max(0, min(ymin, len(timestamps) - 1))

    start_time = datetime.fromtimestamp(timestamps[start_index])
    stop_time = datetime.fromtimestamp(timestamps[stop_index])
    
    IOManager.Exporter.export_spectrogram_to_spectrum(True, current_spectrogram_name, str(tmp_path / "TimeSelectedSpectrum"), "Remark", start_time, stop_time)
    
    assert (tmp_path / "TimeSelectedSpectrum").with_suffix(".xml").is_file()
    
    IOManager.Exporter.export_spectrogram_to_spectrum(True, current_spectrogram_name, str(tmp_path / "FullSpectrum"), "Remark")
    
    assert (tmp_path / "FullSpectrum").with_suffix(".xml").is_file()
    
def test_export_spectrogram_to_xlsx(main_window: MainWindow, qtbot: qtbot_t, tmp_path, test_data):
    parser = file_io.db_parser(test_data / "Debug_Sg.db")
    file_io.db_writer.export_full_xlsx(
        parser, (tmp_path / "SgExport").with_suffix(".xlsx"), True
    )
    assert (tmp_path / "SgExport").with_suffix(".xlsx").is_file()
    parser.close()
    
    
def test_export_spectrogram_to_spectrum(main_window: MainWindow, qtbot: qtbot_t):
    pass
    