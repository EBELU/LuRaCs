import pytest
from pathlib import Path
import time

from PySide6.QtWidgets import QApplication
from pytestqt.qtbot import QtBot

from luracs.main import build_application, MainWindow
from luracs.core.script_engine import ScriptEngine
from luracs.core import Settings


@pytest.fixture
def test_data() -> Path:
    return Path(__file__).parent / "data"


@pytest.fixture(scope="session")
def app_context(
    qapp: QApplication,
    tmp_path_factory,
) -> tuple[QApplication, MainWindow, ScriptEngine]:

    # Create one temporary directory for the entire test session.
    appdata = tmp_path_factory.mktemp("appdata")

    Settings.Paths.set_appdata(appdata)

    app, window, script_engine = build_application()

    assert window is not None

    yield app, window, script_engine

    if window is not None:
        window.close()

@pytest.fixture(scope="session")
def main_window(
    app_context,
) -> MainWindow:
    return app_context[1]


@pytest.fixture(scope="session")
def script_engine(
    app_context,
) -> ScriptEngine:
    return app_context[2]
