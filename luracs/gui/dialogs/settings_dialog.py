from __future__ import annotations
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from main import MainWindow

from PySide6.QtWidgets import (
    QDialog,
    QVBoxLayout,
    QFormLayout,
    QDialogButtonBox,
    QComboBox,
    QRadioButton,
    QButtonGroup,
    QSpinBox,
    QWidget,
    QHBoxLayout,
    QApplication,
    QMessageBox,
)

from PySide6.QtWidgets import QDoubleSpinBox, QCheckBox, QLabel

from luracs.core import Settings, RunManager, log_utils, core_utils
from luracs.utils.color_rotator import ColorRotator


class SettingsDialog(QDialog):
    def __init__(self, name="", title="Settings", parent=None):
        super().__init__(parent=parent)

        self.setWindowTitle(title)
        self.setMinimumWidth(200)

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(8, 8, 8, 8)
        main_layout.setSpacing(6)

        form = QFormLayout()
        form.setSpacing(9)

        # --- Name field ---
        self.theme_combo = QComboBox()
        for theme_option in core_utils.ThemeManager.themes:
            self.theme_combo.addItem(theme_option.value.replace("-", " ").capitalize(), theme_option.value)
        self.theme_combo.setCurrentText(Settings.Appearance.theme.replace("-", " ").capitalize())
        form.addRow("Theme:", self.theme_combo)
        
        self.rotator_combo = QComboBox()
        color_scheme_map = {"lo": "LibreOffice", "mpl": "MatplotLib", "catppuccin": "Catppuccin"}
        for color_scheme in ColorRotator.ColorSchemes:
            self.rotator_combo.addItem(color_scheme_map[color_scheme.value], color_scheme)
        self.rotator_combo.setCurrentText(color_scheme_map[Settings.Appearance.color_rotator_scheme])
        form.addRow("Plot Colour Scheme:", self.rotator_combo)

        self.spectrum_draw_buttons = QButtonGroup()

        pen = QRadioButton("Pen")
        brush = QRadioButton("Brush")
        pen_and_brush = QRadioButton("Pen + Brush")

        self.spectrum_draw_buttons.addButton(pen)
        self.spectrum_draw_buttons.addButton(brush)
        self.spectrum_draw_buttons.addButton(pen_and_brush)

        button_layout = QHBoxLayout()
        button_layout.addWidget(pen)
        button_layout.addWidget(brush)
        button_layout.addWidget(pen_and_brush)

        button_widget = QWidget()
        button_widget.setLayout(button_layout)

        if Settings.Appearance.pen and not Settings.Appearance.brush:
            pen.setChecked(True)

        elif not Settings.Appearance.pen and Settings.Appearance.brush:
            brush.setChecked(True)

        else:
            pen_and_brush.setChecked(True)

        form.addRow("Spectrum Draw Style:", button_widget)
        main_layout.addLayout(form)

        # --- Buttons ---
        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)

        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)

        main_layout.addWidget(buttons)

        self.font_size_spin = QSpinBox()
        self.font_size_spin.setValue(Settings.Appearance.font_size)
        self.font_size_spin.setRange(2, 48)

        form.addRow("Font Size:", self.font_size_spin)

        self.verbose_calculation_logging = QCheckBox("Verbose Calculation Logging")
        self.verbose_calculation_logging.setChecked(
            Settings.Appearance.verbose_calculation_logging
        )
        form.addRow("", self.verbose_calculation_logging)

        self.load_rois_on_import = QCheckBox("Load ROIs on import")
        self.load_rois_on_import.setChecked(Settings.Appearance.load_rois_on_import)
        form.addRow("", self.load_rois_on_import)

        self.load_instrument_on_import = QCheckBox("Load instrument on import")
        self.load_instrument_on_import.setChecked(
            Settings.Appearance.load_instrument_on_import
        )
        form.addRow("", self.load_instrument_on_import)

    def get_values(self):
        theme = self.theme_combo.currentData()

        checked_button = self.spectrum_draw_buttons.checkedButton()
        text = checked_button.text()

        if text == "Pen":
            pen = True
            brush = False
        elif text == "Brush":
            pen = False
            brush = True
        else:  # "Pen + Brush"
            pen = True
            brush = True

        font_size = self.font_size_spin.value()

        return {
            "theme": theme,
            "color_rotator_scheme": self.rotator_combo.currentData().value,
            "pen": pen,
            "brush": brush,
            "font_size": font_size,
            "verbose_calculation_logging": self.verbose_calculation_logging.isChecked(),
            "load_rois_on_import": self.load_rois_on_import.isChecked(),
            "load_instrument_on_import": self.load_instrument_on_import.isChecked(),
        }


def edit_settings(main_window: MainWindow):
    dialog = SettingsDialog()
    res = dialog.exec()

    if res != QDialog.Accepted:
        return

    new_settings = dialog.get_values()

    app = QApplication.instance()
    for key, value in new_settings.items():
        if getattr(Settings.Appearance, key) != value:
            setattr(Settings.Appearance, key, value)

            match key:
                case "theme":
                    core_utils.ThemeManager.apply(core_utils.ThemeManager.themes(value))
                    main_window.spectrum_plot_container.request_redraw()
                    for w in app.allWidgets():
                        w.update()
                        w.repaint()

                case "pen" | "brush":
                    main_window.spectrum_plot_container.request_redraw()

                case "font_size":
                    font = app.font()
                    font.setPointSize(value)  # Change the font size
                    app.setFont(font)
                    for widget in app.allWidgets():
                        f = widget.font()
                        f.setPointSize(value)
                        widget.setFont(f)

                case "verbose_calculation_logging":
                    Settings.Appearance.verbose_calculation_logging = value


class AdvancedSettingsDialog(QDialog):
    def __init__(self, title="Advanced Settings", parent=None):
        super().__init__(parent=parent)

        self.setWindowTitle(title)
        self.setMinimumWidth(300)

        main_layout = QVBoxLayout(self)
        form = QFormLayout()
        form.setSpacing(9)

        # --- Timing ---
        label = QLabel("Device Running")
        label.setStyleSheet("font-weight: bold; margin-top: 10px;")
        form.addRow(label)

        self.update_loop_delay = QDoubleSpinBox()
        self.update_loop_delay.setRange(0.5, 1024)
        self.update_loop_delay.setSingleStep(0.5)
        self.update_loop_delay.setSuffix(" s")
        self.update_loop_delay.setValue(Settings.Advanced.update_loop_delay)
        form.addRow("Update Loop Delay:", self.update_loop_delay)

        self.spectrum_update_delay = QDoubleSpinBox()
        self.spectrum_update_delay.setRange(0.5, 1024)
        self.spectrum_update_delay.setSingleStep(0.5)
        self.spectrum_update_delay.setSuffix(" s")
        self.spectrum_update_delay.setValue(Settings.Advanced.spectrum_update_delay)
        form.addRow("Spectrum Update Delay:", self.spectrum_update_delay)

        # --- Scan lengths ---
        self.ui_scan_length = QSpinBox()
        self.ui_scan_length.setRange(1, 100)
        self.ui_scan_length.setSuffix(" s")
        self.ui_scan_length.setValue(Settings.Advanced.ui_scan_length)
        form.addRow("GUI Scan Length:", self.ui_scan_length)

        self.headless_scan_length = QSpinBox()
        self.headless_scan_length.setRange(1, 100)
        self.headless_scan_length.setSuffix(" s")
        self.headless_scan_length.setValue(Settings.Advanced.headless_scan_length)
        form.addRow("Headless Scan Length:", self.headless_scan_length)

        # --- Optimizer ---
        label = QLabel("Optimizer")
        label.setStyleSheet("font-weight: bold; margin-top: 10px;")
        form.addRow(label)
        self.optimizer_max_iter = QSpinBox()
        self.optimizer_max_iter.setRange(1, 10000)
        self.optimizer_max_iter.setValue(Settings.Advanced.optimizer_max_iter)
        form.addRow("Optimizer Max Iter:", self.optimizer_max_iter)

        self.optimizer_tolerance = QDoubleSpinBox()
        self.optimizer_tolerance.setDecimals(8)
        self.optimizer_tolerance.setRange(1e-9, 1.0)
        self.optimizer_tolerance.setSingleStep(1e-6)
        self.optimizer_tolerance.setValue(Settings.Advanced.optimizer_tolerance)
        form.addRow("Optimizer Tolerance:", self.optimizer_tolerance)

        self.optimizer_use_chi2_weight = QCheckBox()
        self.optimizer_use_chi2_weight.setChecked(
            Settings.Advanced.optimizer_use_chi2_weight
        )
        form.addRow("Use Chi² Weight:", self.optimizer_use_chi2_weight)

        # --- Buffer ---
        label = QLabel("Logging")
        label.setStyleSheet("font-weight: bold; margin-top: 10px;")
        form.addRow(label)
        

        log_row = QWidget()
        log_layout = QHBoxLayout(log_row)
        log_layout.setContentsMargins(0, 0, 0, 0)

        self.log_catch_exceptions = QCheckBox("Log Exceptions")
        self.log_catch_exceptions.setChecked(Settings.Advanced.log_catch_exceptions)

        self.log_write_to_file = QCheckBox("Write logs to file")
        self.log_write_to_file.setChecked(Settings.Advanced.log_write_to_file)

        self.log_write_to_console = QCheckBox("Write logs to console")
        self.log_write_to_console.setChecked(Settings.Advanced.log_write_to_console)

        log_layout.addWidget(self.log_catch_exceptions)
        log_layout.addWidget(self.log_write_to_file)
        log_layout.addWidget(self.log_write_to_console)

        form.addRow("Handlers:", log_row)
        
        self.log_buffer_length = QSpinBox()
        self.log_buffer_length.setRange(10, 100000)
        self.log_buffer_length.setValue(Settings.Advanced.log_buffer_length)
        form.addRow("Headless Log Buffer Length:", self.log_buffer_length)
        
        label = QLabel("Spectrogram")
        label.setStyleSheet("font-weight: bold; margin-top: 10px;")
        form.addRow(label)

        self.spectrogram_deque_length = QSpinBox()
        self.spectrogram_deque_length.setRange(32, 100000)
        self.spectrogram_deque_length.setValue(
            Settings.Advanced.spectrogram_deque_length
        )
        form.addRow("Spectrogram View Buffer:", self.spectrogram_deque_length)

        main_layout.addLayout(form)

        # --- Buttons ---
        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        main_layout.addWidget(buttons)

    def get_values(self):
        return {
            "update_loop_delay": self.update_loop_delay.value(),
            "spectrum_update_delay": self.spectrum_update_delay.value(),
            "ui_scan_length": self.ui_scan_length.value(),
            "headless_scan_length": self.headless_scan_length.value(),
            "optimizer_max_iter": self.optimizer_max_iter.value(),
            "optimizer_tolerance": self.optimizer_tolerance.value(),
            "optimizer_use_chi2_weight": self.optimizer_use_chi2_weight.isChecked(),
            "log_buffer_length": self.log_buffer_length.value(),
            "log_catch_exceptions": self.log_catch_exceptions.isChecked(),
            "log_write_to_file": self.log_write_to_file.isChecked(),       
            "log_write_to_console": self.log_write_to_console.isChecked(),        
            "spectrogram_deque_length": self.spectrogram_deque_length.value(),
        }


def edit_advanced_settings(main_window: MainWindow):
    dialog = AdvancedSettingsDialog()

    if dialog.exec() != QDialog.Accepted:
        return

    new_settings = dialog.get_values()

    require_restart = set(["log_buffer_length"])
    changed_settings = set()

    for key, value in new_settings.items():
        if getattr(Settings.Advanced, key) != value:
            setattr(Settings.Advanced, key, value)
            changed_settings.add(key)

            match key:
                case "spectrogram_deque_length":
                    RunManager.resize_spectrogram_deque(value)
                    
                case "log_catch_exceptions":
                    if value:
                        log_utils.attach_exception_handler()
                    else:
                        log_utils.detach_exception_handler()
                
                case "log_write_to_file":
                    if value:
                        log_utils.attach_file_handler()
                    else:
                        log_utils.detach_file_handler()

                case "log_write_to_console":
                    if value:
                        log_utils.attach_console_handler()
                    else:
                        log_utils.detach_console_handler()

    if len(changed_settings & require_restart):
        QMessageBox.information(
            main_window,
            "Changed Settings Require Restart",
            f"The following settings require a restart of the program to take effect:\n"
            f"{', '.join(changed_settings & require_restart)}",
        )
