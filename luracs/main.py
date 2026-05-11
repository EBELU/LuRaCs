import sys
import asyncio
import logging
from pathlib import Path


def print_progress(text, progress):
    print(
        "\r[" + "#" * progress + " " * (10 - progress) + "]",
        f"-- {text} {' ' * 15}",
        end="",
        flush=True,
    )


logging.basicConfig(level=logging.INFO)

from PySide6.QtGui import QCloseEvent
from PySide6.QtWidgets import (
    QApplication,
    QMainWindow,
    QWidget,
    QVBoxLayout,
    QTabWidget,
)
from qasync import QEventLoop
from PySide6.QtCore import QTimer

print_progress("Loading GUI", 0)

from gui import MainMenuBar, SpectrumPlotContainer, SpectrogramWidget, DataLibrary

print_progress("Loading GUI", 2)

from gui.tabs import (
    ROIInfoTab,
    SpectrumInfoTab,
    LogWidget,
    DevicesInfoTab,
    CurrentValuesPlot,
    IsotopicsTab,
    ConsoleTab
)

print_progress("Loading utils", 5)
from gui.import_export import FileDialogs
from ThemeManager import ThemeManager
from utils.ArgParser import parse_cli_args
from utils.startup import startup_script

from core import RunManager, Log, Settings, SpectrumManager
from utils.file_io.nuclide_dataloader import load_nuclide_data

from gui.popup_windows.BluetoothListPopup import BluetoothListPopup
from gui.popup_windows.USBListPopup import USBListPopup
from gui.popup_windows.documentation_dialogs import SmallDocumentationDialog, DocumentationDialog
from gui.popup_windows.settings_dialog import SettingsDialog
from gui.popup_windows.efficiency_dialog import EfficiencyWindow


__version__ = "0.1.0"
from core.script_engine import ScriptEngine

# ===================== MAIN WINDOW =====================
class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        print_progress("Initializing main window", 7)
        self.setWindowTitle("LuRaCs")

        self.mock_running = True
        self._closing = False

        self.theme = ThemeManager(Settings.Appearance.theme)
        self.theme.apply()

        self.bt_window = BluetoothListPopup()

        self.usb_window = USBListPopup()
        self.file_import_export = FileDialogs(self)
        print_progress("Indexing Data Store", 8)
        load_nuclide_data()
        self.data_store = DataLibrary("Data Store", None)
        
        self.bibliography_dialog = SmallDocumentationDialog(Path("luracs/resources/docs/bibliography.md"))
        self.documentation_dialog = DocumentationDialog(Path("luracs/resources/docs/documentation"), parent=None)
        
        self.settings_dialog = SettingsDialog()
        
        self.calculate_windows = {
            "efficiency": EfficiencyWindow()
        }

        central = QWidget()
        self.setCentralWidget(central)
        layout = QVBoxLayout(central)

        self.menu_bar = MainMenuBar(self)
        self.setMenuBar(self.menu_bar)

        # ---------- SPECTRUM PLOT ----------
        self.spect_tab = QTabWidget()
        self.spect_tab.setTabPosition(QTabWidget.South)
        
        
        self.spectrum_plot_container = SpectrumPlotContainer(self)
        self.spect_tab.addTab(self.spectrum_plot_container, "Spectrum")

        self.spectrogram = SpectrogramWidget(self)
        self.spectrogram.sigShowDataStore.connect(self.show_data_store)
        self.spect_tab.addTab(self.spectrogram, "Spectrogram")

        layout.addWidget(self.spect_tab, 5)

        # ---------- Bottom Tabs ----------

        self.bottom_tabs = QTabWidget(self)

        # Spectrum Infp
        self.spectrum_info_tab = SpectrumInfoTab(parent=self)
        self.bottom_tabs.addTab(self.spectrum_info_tab, "Spectra")

        # ROI info
        self.roi_info_pane = ROIInfoTab(parent=self)
        self.roi_info_pane.clearROIs.connect(SpectrumManager.ROIManager.clear_all)
        self.bottom_tabs.addTab(self.roi_info_pane, "ROI Info")

        # Current values
        self.current_value_tab = CurrentValuesPlot()
        self.bottom_tabs.addTab(self.current_value_tab, "Current Values")

        # Devices
        self.devices_tab = DevicesInfoTab()
        self.bottom_tabs.addTab(self.devices_tab, "Devices")
        
        self.isotopics_tab = IsotopicsTab(list(SpectrumManager.NuclideLibrary.get_sorted_nuclide_names()))
        self.bottom_tabs.addTab(self.isotopics_tab, "Isotopics")
        self.spectrum_plot_container.sigRedrawRequested.connect(self.isotopics_tab.request_line_data)
        self.isotopics_tab.sigColorChanged.connect(lambda : self.spectrum_plot_container.request_redraw())
        self.isotopics_tab.btn_assign_emissions.clicked.connect(self.spectrum_plot_container.match_nuclide_to_rois)
        self.menu_bar.tabbed_action.triggered.connect(self.isotopics_tab.set_search_combo)
        self.menu_bar.combined_action.triggered.connect(self.isotopics_tab.set_search_combo)

        # Console
        self.console_tab = ConsoleTab()
        self.bottom_tabs.addTab(self.console_tab, "Console")

        # System log
        self.log_tab = LogWidget()
        self.bottom_tabs.addTab(self.log_tab, "System Log")

        bottom_layout = QVBoxLayout()
        bottom_layout.addWidget(self.bottom_tabs)
        layout.addLayout(bottom_layout, stretch=3)

        # Theming
        self.theme.register_plot(self.current_value_tab.cps_plot_widget)
        self.theme.register_plot(self.current_value_tab.dose_plot_widget)
        self.theme.register_plot(self.spectrogram.plot)
        self.theme.register_plot(self.spectrogram.top_spectrum_plot)

        self.theme.register_legend(self.current_value_tab.legends)

        self.theme.apply()

        # Run things that need the event loop active
        if len(sys.argv) > 1:                
            QTimer.singleShot(0, lambda: parse_cli_args(self))
            
        if Settings.Appearance.tabbed_spectrum_view:
            self.spectrum_plot_container.set_tabbed_mode()
        else:
            self.spectrum_plot_container.set_combined_mode()


        print_progress("Main window loaded", 9)
        
        
        

    def show_data_store(self, tab_idx=None):
        if tab_idx is not None:
            self.data_store.tabs.setCurrentIndex(tab_idx)

        self.data_store.show()

    def closeEvent(self, event: QCloseEvent):
        if self._closing:
            event.accept()
            return
        Log.info("Disconnecting devices and shutting down application...")
        event.ignore()
        Settings.save_settings()
        self.hide()
        asyncio.create_task(self._async_close())

    async def _async_close(self):
        if self._closing:
            return
        self._closing = True

        try:
            await RunManager.shutdown()
            Settings.save_settings()
        finally:
            QApplication.quit()


# ===================== ENTRY =====================
def main():
    startup_script()
    app = QApplication(sys.argv)
    app.setStyle("Fusion")
    font = app.font()
    font.setPointSize(Settings.Appearance.font_size)  # Change the font size
    app.setFont(font)
    loop = QEventLoop(app)
    asyncio.set_event_loop(loop)

    win = MainWindow()
    
    # Check if headless
    if "--headless" in sys.argv:
        Settings.headless = True
    else:
        # If not headless, show the GUI
        win.show()

    RunManager.set_loop(loop)

    # Script engine
    script_engine = ScriptEngine(program_version = __version__, headless=Settings.headless)
    def on_quit():
        script_engine.submit_from_sync("__exit__")
    app.aboutToQuit.connect(on_quit)

    win.console_tab.sigCommandEntered.connect(script_engine.submit_from_sync)
    script_engine.sigCommandAppendOutput.connect(win.console_tab.append_output)
    script_engine.sigCommandOutput.connect(win.console_tab.set_output)
    script_engine.sigShutdown.connect(app.quit)
    script_engine.connect_log_buffer(win.log_tab.get_buffered_logs)

    Log.info(
f"""

 ======  ======  ======     
|71    ||88    ||55    |    Version:  {__version__} \t [2026-04-14]
|  Lu  ||  Ra  ||  Cs  |    Licence:  GNU General Public Licence v3.0
| 177  || 226  || 137  |    
 ======  ======  ======     
""")
    print_progress("Done!", 10)
    print()
    # Start the event loop
    with loop:
        loop.create_task(script_engine.start())
        loop.run_forever()

if __name__ == "__main__":
    main()
