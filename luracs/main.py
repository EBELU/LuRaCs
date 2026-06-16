import sys
import asyncio
import logging
__version__ = "0.2.0"


def print_progress(text, progress):
    print(
        "\r[" + "#" * progress + " " * (10 - progress) + "]",
        f"-- {text} {' ' * 15}",
        end="",
        flush=True,
    )

logging.basicConfig(level=logging.INFO if "-db" in sys.argv else logging.INFO)


# --- PySide6 Imports for main window ---
from PySide6.QtGui import QCloseEvent, QIcon
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

# --- Perform standard internal imports ---
from luracs.core import RunManager, Log, Settings, SpectrumManager, log_utils, core_utils
from luracs.core.script_engine import ScriptEngine # Not normally exposed in the api

from luracs.gui import MainMenuBar, SpectrumPlotContainer, SpectrogramWidget

from luracs.gui.tabs import (
    ROIInfoTab,
    SpectrumInfoTab,
    LogWidget,
    DevicesInfoTab,
    RealTimeValuesPlot,
    IsotopicsTab,
    ConsoleTab,
)

from luracs.gui.windows import (
    BluetoothListPopup,
    USBListPopup,
    DataLibrary,
    SmallDocumentationDialog,
    DocumentationDialog,
    CalibrationWindow,
    EfficiencyWindow,
    ResolutionWindow,
)

from luracs.gui.dialogs.settings_dialog import SettingsDialog

# --- Import heavy features excluded in the lightweight version ---
from luracs.config import IS_H3
if not IS_H3:
    pass

print_progress("Loading luracs.utils.", 5)

from luracs.theme_manager import ThemeManager
from luracs.utils.arg_parser import parse_cli_args
from luracs.utils.startup import startup_script



# ===================== IMPORTANT GUI CONNECTIONS =====================

# Connect the edit dialog from the GUI to the core ROI object
# If its not done like this we have circular import hell!
from luracs.gui.dialogs.roi_editor import ROIEditor
from luracs.containers.roi_classes import DeletableROI

DeletableROI.roi_editor_dialog = ROIEditor



# ===================== IMPORTANT SIGNALS =====================

RunManager.createDeviceSpectrum.connect(SpectrumManager.create_spectrum)
RunManager.removeDeviceSpectrum.connect(SpectrumManager.remove_spectrum)
RunManager.spectrumUpdated.connect(SpectrumManager.set_foreground_spectrum)



# ===================== MAIN WINDOW =====================
class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        print_progress("Initializing main window", 7)
        self.setWindowTitle("LuRaCs" if not IS_H3 else "LuRaCs-H3")

        self.mock_running = True
        self._closing = False
        
        # Shown from main menu bar
        self.bt_window = BluetoothListPopup()
        self.usb_window = USBListPopup()
        
        print_progress("Building GUI", 8)
        self.data_store = DataLibrary("Data Store", None)

        self.bibliography_dialog = SmallDocumentationDialog(Settings.Paths.bibliography.as_posix())
        self.documentation_dialog = DocumentationDialog(
            Settings.Paths.documentation_dir.as_posix(), parent=None
        )

        self.settings_dialog = SettingsDialog()

        self.calc_win_efficiency = EfficiencyWindow()
        self.calc_win_calibration = CalibrationWindow()
        self.calc_win_resolution = ResolutionWindow()

        central = QWidget()
        self.setCentralWidget(central)
        layout = QVBoxLayout(central)

        self.main_menu_bar = MainMenuBar(self)
        self.setMenuBar(self.main_menu_bar)

        # ---------- SPECTRUM PLOT ----------
        self.spect_tab = QTabWidget()
        self.spect_tab.setTabPosition(QTabWidget.South)
        self.spect_tab.setObjectName("southTabs")

        self.spectrum_plot_container = SpectrumPlotContainer(self)
        self.spect_tab.addTab(self.spectrum_plot_container, "Spectrum")

        self.spectrogram = SpectrogramWidget(self)
        self.spectrogram.sigShowDataStore.connect(self.show_data_store)
        self.spect_tab.addTab(self.spectrogram, "Spectrogram")

        layout.addWidget(self.spect_tab, 5)

        # ---------- Bottom Tabs ----------

        self.bottom_tabs = QTabWidget(self)

        # Spectrum Infp
        self.spectrum_info_tab = SpectrumInfoTab(self, parent=self)
        self.bottom_tabs.addTab(self.spectrum_info_tab, "Spectrum Info")

        # ROI info
        self.roi_info_pane = ROIInfoTab(parent=self)
        self.roi_info_pane.clearROIs.connect(SpectrumManager.ROIManager.clear_all)
        self.bottom_tabs.addTab(self.roi_info_pane, "ROI Info")

        # Current values
        self.current_value_tab = RealTimeValuesPlot()
        self.bottom_tabs.addTab(self.current_value_tab, "Real Time Values")

        # Devices
        self.devices_tab = DevicesInfoTab()
        self.bottom_tabs.addTab(self.devices_tab, "Devices")

        # Isotopics
        self.isotopics_tab = IsotopicsTab(
            list(SpectrumManager.NuclideLibrary.get_sorted_nuclide_names())
        )
        self.bottom_tabs.addTab(self.isotopics_tab, "Isotopics")
        # Isotopics connections
        self.spectrum_plot_container.sigRedrawRequested.connect(
            self.isotopics_tab.request_line_data
        )  # If spectrum plot redraws, get nuclide lines
        self.spectrum_plot_container.sigTabChanged.connect(
            self.isotopics_tab.search_spect_combo.setCurrentText
        )  # In tabbed mode change what spectrum is searched by selected tab

        self.isotopics_tab.sigColorChanged.connect(
            lambda: self.spectrum_plot_container.request_redraw()
        )  # Redraw on color change
        self.isotopics_tab.btn_assign_emissions.clicked.connect(
            self.spectrum_plot_container.match_nuclide_to_rois
        )

        self.main_menu_bar.tabbed_action.triggered.connect(
            self.isotopics_tab.set_search_combo
        )  # Disable combo
        self.main_menu_bar.combined_action.triggered.connect(
            self.isotopics_tab.set_search_combo
        )  # Enable combo

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
        core_utils.ThemeManager.register_plot(self.current_value_tab.cps_plot_widget)
        core_utils.ThemeManager.register_plot(self.current_value_tab.dose_plot_widget)
        core_utils.ThemeManager.register_plot(self.spectrogram.plot)
        core_utils.ThemeManager.register_plot(self.spectrogram.top_spectrum_plot)
        core_utils.ThemeManager.register_plot(self.calc_win_efficiency.demo_plot)
        core_utils.ThemeManager.register_plot(self.calc_win_calibration.calibration_plot)
        core_utils.ThemeManager.register_plot(self.calc_win_resolution.res_plot)
        core_utils.ThemeManager.register_legend(*self.current_value_tab.legends)
        core_utils.ThemeManager.register_legend(self.spectrum_plot_container.single_plot.legend)
        core_utils.ThemeManager.apply(ThemeManager.themes(Settings.Appearance.theme))

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

    def closeEvent(self, event: QCloseEvent = None):
        if self._closing:
            event.accept()
            return
        Log.info("Disconnecting devices and shutting down application...")
        event.ignore()
        self.hide()
        Settings.save_settings()
        asyncio.create_task(self._async_close())

    async def _async_close(self):
        if self._closing:
            return
        self._closing = True
        try:
            await RunManager.shutdown()
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
    app.setWindowIcon(QIcon(str(Settings.Paths.themes / "icons" / "main_icon_green.png")))
    loop = QEventLoop(app)
    asyncio.set_event_loop(loop)

    win = MainWindow()

    # Check if headless
    if "--headless" in sys.argv:
        Settings.headless = True
    else:
        # If not headless, show the GUI
        win.show()

    # --- Set Handlers for the logger ---
    if Settings.Advanced.log_write_to_file:
        log_utils.attach_file_handler()

    if Settings.Advanced.log_catch_exceptions:
        log_utils.attach_exception_handler()

    if Settings.Advanced.log_write_to_console and not Settings.headless:
        log_utils.attach_console_handler()


    # --- Script engine ---
    script_engine = ScriptEngine(
        program_version=__version__, headless=Settings.headless
    )

    # Shutdown
    def on_quit():
        script_engine.submit_from_sync("__exit__")

    app.aboutToQuit.connect(on_quit)

    # Connect Signals
    win.console_tab.sigCommandEntered.connect(script_engine.submit_from_sync)
    script_engine.sigCommandAppendOutput.connect(win.console_tab.append_output)
    script_engine.sigCommandOutput.connect(win.console_tab.append_output)
    script_engine.sigClearConsole.connect(win.console_tab.set_output)
    script_engine.sigShutdown.connect(lambda: asyncio.create_task(win._async_close()))
    script_engine.connect_log_buffer(win.log_tab.get_buffered_logs)

    # --- Log welcome ---
    Log.info(
        "\n"
        "\n ======  ======  ====== "    
       f"\n|71    ||88    ||55    |    Version:  {__version__} \t [2026-04-14]"
       f"\n|  Lu  ||  Ra  ||  Cs  |    Licence:  GNU General Public Licence v3.0"
        "\n| 177  || 226  || 137  |"
        "\n ======  ======  ====== "    
    )
    print_progress("Done!", 10)
    print()

    # --- Start the event loop ---
    with loop:
        loop.create_task(script_engine.start())
        loop.run_forever()


if __name__ == "__main__":
    main()
