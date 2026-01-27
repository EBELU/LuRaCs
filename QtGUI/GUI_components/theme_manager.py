from PySide6.QtGui import QPalette, QColor
from PySide6.QtCore import Qt
from PySide6.QtWidgets import QApplication
import pyqtgraph as pg


class ThemeManager:
    DARK = "dark"
    LIGHT = "light"

    def __init__(self, mode=LIGHT):
        self.mode = mode

    # ---------- Public API ----------

    def toggle(self, plot_widgets=None):
        self.mode = self.LIGHT if self.mode == self.DARK else self.DARK
        self.apply(plot_widgets)

    def apply(self, plot_widgets=None):
        app = QApplication.instance()
        if not app:
            return

        self._apply_qt_palette(app)
        self._apply_pg_globals()

        if plot_widgets:
            for pw in plot_widgets:
                self._style_plot_widget(pw)

    # ---------- Qt ----------

    def _apply_qt_palette(self, app):
        if self.mode == self.DARK:
            app.setPalette(self._dark_palette())
        else:
            app.setPalette(QPalette())
    def _dark_palette(self):
        p = QPalette()

        # ---- Enabled / Normal ----
        p.setColor(QPalette.Window, QColor(30, 30, 30))
        p.setColor(QPalette.WindowText, Qt.white)
        p.setColor(QPalette.Base, QColor(25, 25, 25))
        p.setColor(QPalette.AlternateBase, QColor(35, 35, 35))
        p.setColor(QPalette.Text, Qt.white)
        p.setColor(QPalette.Button, QColor(45, 45, 45))
        p.setColor(QPalette.ButtonText, Qt.white)
        p.setColor(QPalette.Highlight, QColor(90, 140, 200))
        p.setColor(QPalette.HighlightedText, Qt.black)

        # ---- Disabled ----
        p.setColor(QPalette.Disabled, QPalette.Button, QColor(35, 35, 35))
        p.setColor(QPalette.Disabled, QPalette.ButtonText, QColor(130, 130, 130))
        p.setColor(QPalette.Disabled, QPalette.WindowText, QColor(130, 130, 130))
        p.setColor(QPalette.Disabled, QPalette.Text, QColor(120, 120, 120))
        p.setColor(QPalette.Disabled, QPalette.Highlight, QColor(60, 60, 60))
        p.setColor(QPalette.Disabled, QPalette.HighlightedText, QColor(150, 150, 150))

        return p

    # ---------- pyqtgraph ----------

    def _apply_pg_globals(self):
        if self.mode == self.DARK:
            pg.setConfigOption("background", (30, 30, 30))
            pg.setConfigOption("foreground", "w")
        else:
            pg.setConfigOption("background", "w")
            pg.setConfigOption("foreground", "k")

    def _style_plot_widget(self, pw):
        # Background only
        pw.setBackground((30, 30, 30) if self.mode == self.DARK else "w")

        # Axes only
        axis_pen = pg.mkPen("w" if self.mode == self.DARK else "k")
        for axis in ("left", "bottom", "right", "top"):
            ax = pw.getAxis(axis)
            if ax:
                ax.setPen(axis_pen)
                ax.setTextPen(axis_pen)

        # Grid only
        pw.showGrid(x=True, y=True, alpha=0.3 if self.mode == self.DARK else 0.4)
