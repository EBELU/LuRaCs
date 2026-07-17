import sys
import asyncio
import logging

__version__ = "0.1.1"


def print_progress(text, progress):
    print(
        "\r[" + "=" * progress + " " * (10 - progress) + "]",
        f"-- {text} {' ' * 15}",
        end="",
        flush=True,
    )


logging.basicConfig(level=logging.INFO)

# --- Vital imports for core application to function ---
from PySide6.QtWidgets import QApplication
from qasync import QEventLoop
from luracs.core import (
    RunManager,
    Log,
    Settings,
    SpectrumManager,
    log_utils,
    core_utils,
)
from luracs.core.script_engine import ScriptEngine  # Not normally exposed in the api

from luracs.utils.arg_parser import parse_cli_args
from luracs.utils.startup import startup_script
from luracs.utils import ascii_art


# --- PySide6 Imports for main window ---
from PySide6.QtGui import QCloseEvent, QIcon
from PySide6.QtWidgets import (
    QMainWindow,
    QWidget,
    QVBoxLayout,
    QTabWidget,
)

from PySide6.QtCore import QTimer

print_progress("Loading GUI", 0)

# --- Perform standard internal imports ---
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
from luracs.theme_manager import ThemeManager

# --- Import heavy features excluded in the lightweight version ---
try:
    import PySide6.QtWebEngineCore

    IS_H3 = False
except ModuleNotFoundError:
    IS_H3 = True


if not IS_H3:
    from luracs.gui.mapping import MapWidget

print_progress("Loading luracs.utils.", 5)

# ===================== IMPORTANT GUI CONNECTIONS =====================

# Connect the edit dialog from the GUI to the core ROI object
# If its not done like this we have circular import hell!
from luracs.gui.dialogs.roi_editor import ROIEditor
from luracs.containers.roi_classes import DeletableROI

DeletableROI.roi_editor_dialog = ROIEditor


# ===================== IMPORTANT SIGNALS =====================

RunManager.Signals.createDeviceSpectrum.connect(SpectrumManager.create_spectrum)
RunManager.Signals.removeDeviceSpectrum.connect(SpectrumManager.remove_spectrum)
RunManager.Signals.spectrumUpdated.connect(SpectrumManager.set_foreground_spectrum)
Settings.sigSettingChanged.connect(
    lambda group, variable, new_value: Log.debug(
        f"{Settings.__class__}: Setting updated: {group}.{variable} = {new_value}"
    )
)


_closing = False


async def _async_close():
    global _closing
    if _closing:
        return
    _closing = True
    try:
        await RunManager.shutdown()
    finally:
        QApplication.quit()


def close():
    Settings.save_settings()
    asyncio.create_task(_async_close())


# ===================== MAIN WINDOW =====================
class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        print_progress("Initializing main window", 7)
        self.setWindowTitle("LuRaCs" if not IS_H3 else "LuRaCs-H3")

        self.data_store = DataLibrary("Data Store", None)

        # Shown from main menu bar
        self.bt_window = BluetoothListPopup()
        self.usb_window = USBListPopup()

        print_progress("Building GUI", 8)

        self.bibliography_dialog = SmallDocumentationDialog(Settings.Paths.bibliography)
        self.documentation_dialog = DocumentationDialog(
            Settings.Paths.documentation_dir, parent=None
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

        if not IS_H3:
            self.map_widget = MapWidget()
            self.spect_tab.addTab(self.map_widget, "Map")
        else:
            self.map_widget = None

        layout.addWidget(self.spect_tab, 6)

        # ---------- Bottom Tabs ----------

        self.bottom_tabs = QTabWidget(self)

        # Spectrum Infp
        self.spectrum_info_tab = SpectrumInfoTab(self, parent=self)
        self.bottom_tabs.addTab(self.spectrum_info_tab, "Spectrum Info")
        self.bottom_tabs.setTabToolTip(
            0, "View detailed information about the spectra currently loaded"
        )

        # ROI info
        self.roi_info_pane = ROIInfoTab(parent=self)
        self.roi_info_pane.clearROIs.connect(SpectrumManager.ROIManager.clear_all)
        self.bottom_tabs.addTab(self.roi_info_pane, "ROI Info")
        self.bottom_tabs.setTabToolTip(
            1,
            "View detailed information about regions of interest (ROI) set in the spectra",
        )

        # Current values
        self.current_value_tab = RealTimeValuesPlot()
        self.bottom_tabs.addTab(self.current_value_tab, "Real Time Values")
        self.bottom_tabs.setTabToolTip(
            2, "View the current values measured by a connected device"
        )
        self.main_menu_bar.view_menu_realtime_avg_line.triggered.connect(
            self.current_value_tab.toggle_mean_lines
        )

        # Devices
        self.devices_tab = DevicesInfoTab()
        self.bottom_tabs.addTab(self.devices_tab, "Devices")
        self.bottom_tabs.setTabToolTip(3, "View connected devices and their status")

        # Isotopics
        self.isotopics_tab = IsotopicsTab(
            list(SpectrumManager.NuclideLibrary.get_sorted_nuclide_names())
        )
        self.bottom_tabs.addTab(self.isotopics_tab, "Isotopics")
        self.bottom_tabs.setTabToolTip(
            4,
            "View radionuclide information, set help lines in the spectrum, search peaks and auto assign peaks",
        )
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
        self.bottom_tabs.setTabToolTip(
            5, "Enter text commands and run scripts in the console"
        )

        # System log
        self.log_tab = LogWidget()
        self.bottom_tabs.addTab(self.log_tab, "System Log")
        self.bottom_tabs.setTabToolTip(6, "View messages logged by the system")

        bottom_layout = QVBoxLayout()
        bottom_layout.addWidget(self.bottom_tabs)
        layout.addLayout(bottom_layout, stretch=3)

        # Theming
        core_utils.ThemeManager.register_plot(
            self.current_value_tab.cps_plot_widget,
            self.current_value_tab.dose_plot_widget,
            self.spectrogram.plot,
            self.spectrogram.top_spectrum_plot,
            self.calc_win_efficiency.demo_plot,
            self.calc_win_calibration.calibration_plot,
            self.calc_win_resolution.res_plot,
        )
        core_utils.ThemeManager.register_legend(
            *self.current_value_tab.legends
            )
        core_utils.ThemeManager.register_legend(
            self.spectrum_plot_container.single_plot.legend
        )
        core_utils.ThemeManager.apply(ThemeManager.themes(Settings.Appearance.theme))
        Log.debug(f"{self.__class__}: Theme loaded '{Settings.Appearance.theme}'")

        # Run things that need the event loop active

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
        global _closing
        if _closing:
            event.accept()
            return
        Log.info("Disconnecting devices and shutting down application...")
        event.ignore()
        self.hide()
        if self.map_widget is not None:
            self.map_widget.stop()
        close()


# ===================== ENTRY =====================
def main():
    startup_script()
    app = QApplication(sys.argv)
    app.setStyle("Fusion")
    font = app.font()
    font.setPointSize(Settings.Appearance.font_size)  # Change the font size
    app.setFont(font)
    app.setWindowIcon(
        QIcon(str(Settings.Paths.themes / "icons" / "main_icon_green.png"))
    )
    loop = QEventLoop(app)
    asyncio.set_event_loop(loop)

    # Check if headless
    if "--headless" in sys.argv:
        Settings.headless = True
        win = None
    else:
        # If not headless, show the GUI
        win = MainWindow()
        win.show()
    Log.setLevel(level=logging.DEBUG if "-db" in sys.argv else logging.INFO)
    Log.debug(f"headless: {Settings.headless}")

    # --- Set Handlers for the logger ---
    if Settings.Advanced.log_write_to_file:
        log_utils.attach_file_handler()

    if Settings.Advanced.log_catch_exceptions:
        log_utils.attach_exception_handler()

    if Settings.Advanced.log_write_to_console and not Settings.headless:
        log_utils.attach_console_handler()

    # --- Script engine ---
    script_engine = ScriptEngine(
        program_version=__version__ + "--Tritium" if IS_H3 else __version__,
        headless=Settings.headless,
        IS_H3=IS_H3,
    )

    # Shutdown
    def on_quit():
        script_engine.submit_from_sync("__exit__")

    app.aboutToQuit.connect(on_quit)

    # Connect Signals
    script_engine.sigShutdown.connect(lambda: asyncio.create_task(_async_close()))
    script_engine.connect_log_buffer(log_utils.log_buffer.get_messages)
    if win is not None:  # Does the main window exist?
        win.console_tab.sigCommandEntered.connect(script_engine.submit_from_sync)
        script_engine.sigCommandAppendOutput.connect(win.console_tab.append_output)
        script_engine.sigCommandOutput.connect(win.console_tab.append_output)
        script_engine.sigClearConsole.connect(win.console_tab.set_output)

        script_engine.sigMapURL.connect(win.map_widget.load_map_from_url)
        script_engine.sigMapFile.connect(win.map_widget.load_offline_map)

    print_progress("Done!", 10)
    print()
    # --- Log welcome ---
    Log.info(
        "\n\n"
        + ascii_art.logo(
            __version__ + "--Tritium" if IS_H3 else __version__,
            is_h3=IS_H3,
            use_type="log",
        )
    )

    # --- Handle Command Line Arguments ---
    if len(sys.argv) > 1:
        QTimer.singleShot(0, lambda: parse_cli_args(win))

    # --- Start the event loop ---
    with loop:
        loop.create_task(script_engine.start())
        loop.run_forever()


if __name__ == "__main__":
    main()
