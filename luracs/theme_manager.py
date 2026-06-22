from PySide6.QtGui import QColor
from PySide6.QtWidgets import QApplication
from PySide6.QtCore import QObject, Signal
import pyqtgraph as pg
from enum import Enum
import json
from pathlib import Path


class ThemeManager(QObject):
    """Controls the theme of the app."""
    sigUpdateSettingsTheme = Signal(str, str, str)
    class themes(Enum):
        LIGHT = "light"
        LIGHT_CATPPUCCIN = "light-catppuccin"
        DARK_CATPPUCCIN = "dark-catppuccin"

    _registry_plots = []
    _registry_legends = []
    _registry_hist_lut = []

    def __init__(self, themes_path: Path, mode=themes.LIGHT):
        super().__init__()
        self.mode = mode
        self.themes_path = themes_path
        
    # ---------- Public API ----------
    def register_plot(self, *plots):
        """Register one or more plot widgets."""
        self._registry_plots.extend(plots)

    def unregister_plot(self, *plots):
        """Remove one or more plot widgets."""
        for plot in plots:
            if plot in self._registry_plots:
                self._registry_plots.remove(plot)

    def register_legend(self, *legends):
        """Register one or more legend items."""
        self._registry_legends.extend(legends)

    def unregister_legend(self, *legends):
        """Remove one or more legend items."""
        for legend in legends:
            if legend in self._registry_legends:
                self._registry_legends.remove(legend)

    def register_hist_lut(self, *luts):
        """Register one or more histogram LUT widgets."""
        self._registry_hist_lut.extend(luts)

    def unregister_hist_lut(self, *luts):
        """Remove one or more histogram LUT widgets."""
        for lut in luts:
            if lut in self._registry_hist_lut:
                self._registry_hist_lut.remove(lut)


    def apply(self, style_name: themes):
        self.theme = style_name
        self.sigUpdateSettingsTheme.emit("Appearance", "theme", style_name.value)
        theme_is_dark = True if "dark" in style_name.value.lower() else False

        if style_name == self.themes.LIGHT:
            self.colors = {
                "base": QColor(255, 255, 255),
                "text": QColor(0, 0, 0),
                "surface0": QColor(255, 255, 255),
            }

        else:
            style_sheet, self.colors = self._load_styles(style_name)

            app = QApplication.instance()
            if not app:
                return

            app.setStyleSheet(style_sheet)

        pg.setConfigOption("background", self.colors["base"])
        pg.setConfigOption("foreground", self.colors["text"])

        for pw in self._registry_plots:
            self._style_plot_widget(
                pw, self.colors["text"], self.colors["base"], theme_is_dark
            )

        for lgd in self._registry_legends:
            self._style_legend(
                lgd, self.colors["text"], self.colors["surface0"], theme_is_dark
            )
        
        for lut in self._registry_hist_lut:
            self._style_hist_lut(
                lut, self.colors["text"]
            )
            
    def _load_styles(self, style_name: themes):
        style_sheet_pth = self.themes_path / "variants" / style_name.value
        with open(str(style_sheet_pth.with_suffix(".css")), "r") as f:
            style_sheet = f.read()
            style_sheet = style_sheet.replace(
                "${THEMES_PATH}", str(self.themes_path.as_posix())
            )

        with open(str(style_sheet_pth.with_suffix(".json")), "r") as f:
            colors = json.load(f)

        return style_sheet, colors

    def _style_plot_widget(self, pw, foreground, background, theme_is_dark):
        try:
            pw.setBackground(background)
        except AttributeError:
            pw.getViewBox().setBackgroundColor(background)
            pw.getViewWidget().setBackgroundBrush(pg.mkBrush(background))

        except RuntimeError:
            return

        axis_pen = pg.mkPen(foreground)
        for axis in ("left", "bottom", "right", "top"):
            ax = pw.getAxis(axis)
            if ax:
                ax.setPen(axis_pen)
                ax.setTextPen(axis_pen)

        pw.showGrid(x=True, y=True, alpha=0.2 if theme_is_dark else 0.3)
        pw.enableAutoRange()

    def _style_legend(
        self, legend: pg.LegendItem, foreground, background, theme_is_dark
    ):
        if isinstance(background, tuple):
            bg_color = QColor(*background)
        else:
            bg_color = QColor(background)
        bg_color.setAlpha(100 if theme_is_dark else 220)
        legend.setBrush(pg.mkBrush(bg_color))

        for sample, label in legend.items:
            label.setText(label.text, color=foreground)
            if hasattr(sample, "setPen"):
                sample.setPen(pg.mkPen(foreground))
            if hasattr(sample, "setBrush"):
                sample.setBrush(pg.mkBrush(foreground))
                
    def _style_hist_lut(self, hist, foreground):
        # 1. Axis ticks + labels
        hist.axis.setPen(pg.mkPen(foreground))
        hist.axis.setTextPen(pg.mkPen(foreground))

        # 3. Gradient ticks (important)
        hist.gradient.tickPen = pg.mkPen(foreground)
        hist.gradient.textPen = pg.mkPen(foreground)

        # Force redraw
        hist.gradient.update()
