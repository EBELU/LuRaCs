import sys
import asyncio
import logging

logging.basicConfig(
    level=logging.INFO
)

from PySide6.QtGui import QCloseEvent
from PySide6.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, QTabWidget
)
from qasync import QEventLoop
from PySide6.QtCore import QTimer



from GUI_components import (MainMenuBar, 
                            SpectrumPlot, 
                            SpectrogramWidget)

from GUI_components.tabs import(ROIInfoTab,
                                SpectrumInfoTab,
                                LogWidget,
                                DevicesInfoTab,
                                CurrentValuesPlot)


from GUI_components.import_export import FileDialogs
from ThemeManager import ThemeManager
from utils.ArgParser import parse_cli_args
from utils.startup import startup_script

from core import RunManager, Log, Settings, GuiServices, GuiServicesKeys

from GUI_components.popup_windows.BluetoothListPopup import BluetoothListPopup
from GUI_components.popup_windows.USBListPopup import USBListPopup

from PySide6.QtWidgets import QApplication




# ===================== MAIN WINDOW =====================
class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        
        self.setWindowTitle("Gamma Spectroscopy")

        self.mock_running = True
        self._closing = False
        
        self.theme = ThemeManager(Settings.Appearance.theme)
        self.theme.apply() 
        
        self.bt_window = BluetoothListPopup()
        
        self.usb_window = USBListPopup()
        self.file_import_export = FileDialogs(self)

        central = QWidget()
        self.setCentralWidget(central)
        layout = QVBoxLayout(central)

        self.menu_bar = MainMenuBar(self)
        self.setMenuBar(self.menu_bar)


        # ---------- SPECTRUM PLOT ----------
        self.spect_tab = QTabWidget()
        self.spect_tab.setTabPosition(QTabWidget.South)
        self.spectrum_plot = SpectrumPlot()
        self.spect_tab.addTab(self.spectrum_plot, "Spectrum")
        
        self.spectrogram = SpectrogramWidget()
        self.spect_tab.addTab(self.spectrogram, "Spectrogram")
        
        layout.addWidget(self.spect_tab, 5)

        # ---------- Bottom Tabs ----------

        self.bottom_tabs = QTabWidget(self)
 
        # Spectrum Infp
        self.spectrum_info_tab = SpectrumInfoTab(parent=self)
        self.bottom_tabs.addTab(self.spectrum_info_tab, "Spectra")

        # ROI info
        self.roi_info_pane = ROIInfoTab(parent=self)
        self.roi_info_pane.clearROIs.connect(self.spectrum_plot._clear_rois)
        self.bottom_tabs.addTab(self.roi_info_pane, "ROI Info")

        # Current values
        self.current_value_tab = CurrentValuesPlot()
        self.bottom_tabs.addTab(self.current_value_tab, "Current Values")

        # Devices
        self.devices_tab = DevicesInfoTab()
        self.bottom_tabs.addTab(self.devices_tab, "Devices")

        # System log
        self.log_tab = LogWidget()
        self.bottom_tabs.addTab(self.log_tab, "System Log")


        bottom_layout = QVBoxLayout()
        bottom_layout.addWidget(self.bottom_tabs)
        layout.addLayout(bottom_layout, stretch=3)
        

        self.theme.apply(plot_widgets=[
            self.spectrum_plot.plot_widget, 
            self.current_value_tab.cps_plot_widget,
            self.current_value_tab.dose_plot_widget,
            ],
            legends=self.current_value_tab.legends)
        
        if len(sys.argv) > 1:
            QTimer.singleShot(0, parse_cli_args)
            
            
    def closeEvent(self, event: QCloseEvent):
        if self._closing:
            event.accept()
            return
        Log.info("Disconnecting devices and shutting down application...")
        event.ignore()
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
    font = app.font()
    font.setPointSize(Settings.Appearance.font_size)  # Change the font size
    app.setFont(font)
    loop = QEventLoop(app)
    asyncio.set_event_loop(loop)
    


    win = MainWindow()
    win.show()

    RunManager.set_loop(loop)

    header = "[ APPLICATION STARTED ]"
    version = "Version: Alpha"
    platform = f"Platform: {sys.platform}"

    frame_width = max(len(header), len(version), len(platform)) + 4  # extra padding

    line = "=" * frame_width

    Log.info(f"\n{line}\n"
            f"| {header.center(frame_width-4)} |\n"
            f"| {version.center(frame_width-4)} |\n"
            f"| {platform.center(frame_width-4)} |\n"
            f"{line}")

    # Start the event loop
    with loop:
        loop.run_forever()


if __name__ == "__main__":
    main()



# pyinstaller \
#     --name MySpect \
#     --onedir \
#     --exclude-module PySide6.QtNetwork \
#     --exclude-module PySide6.Qt3DCore \
#     --exclude-module PySide6.Qt3DRender \
#     --exclude-module PySide6.Qt3DExtras \
#     --exclude-module PySide6.QtMultimedia \
#     --exclude-module PySide6.QtWebEngineWidgets \
#     --exclude-module scipy.spatial \
#     --exclude-module scipy.linalg \
#     --exclude-module scipy.ndimage \
#     --exclude-module matplotlib \
#     --hidden-import scipy.optimize \
#     --hidden-import bleak \
#     GUI_main.py