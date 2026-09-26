from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pytestqt import qtbot as qtbot_t

    from luracs.main import MainWindow
    
from luracs.core import IOManager, SpectrumManager, RunManager
from luracs.utils import file_io
import pytest

pytestmark = pytest.mark.order(2)

def test_spectrogram_start(script_engine, qtbot):
    script_engine.submit_from_sync("spectrogram start all")
    qtbot.wait(2000)
    
    assert len(RunManager.SpectrogramManager.spectrogram_registry)
    
def test_load_spectrogram(test_data):
    pass
    
def test_spectrogram_time_selector(main_window: MainWindow, qtbot: qtbot_t):
    main_window.spectrogram.action_time_selector.trigger()
    qtbot.wait(500)
    main_window.spectrogram.action_time_selector.trigger()
    
    