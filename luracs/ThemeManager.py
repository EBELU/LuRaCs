import sys
from PySide6.QtGui import QPalette, QColor
from PySide6.QtWidgets import QApplication
import pyqtgraph as pg


class ThemeManager:
    """Controls the theme of the app.

    Can switch between LIGHT and DARK modes.
    Keeps a registry of plot widgets and legends to auto-apply styling.
    """

    DARK = "dark"
    LIGHT = "light"

    _registry_plots = []
    _registry_legends = []

    def __init__(self, mode=LIGHT):
        self.mode = mode

    # ---------- Public API ----------

    def register_plot(self, plot):
        """Register a plot widget or a list of plot widgets."""
        if isinstance(plot, list):
            self._registry_plots.extend(plot)
        else:
            self._registry_plots.append(plot)
            
    def unregister_plot(self, plot):
        """Remove a plot widget or list of plot widgets."""
        if isinstance(plot, list):
            for p in plot:
                if p in self._registry_plots:
                    self._registry_plots.remove(p)
        else:
            if plot in self._registry_plots:
                self._registry_plots.remove(plot)

    def register_legend(self, legend):
        """Register a legend item or a list of legend items."""
        if isinstance(legend, list):
            self._registry_legends.extend(legend)
        else:
            self._registry_legends.append(legend)

    def toggle(self):
        """Toggle the theme and re-apply."""
        self.mode = self.LIGHT if self.mode == self.DARK else self.DARK
        self.apply()

    def apply(self):
        """Apply the theme to the app, registered plots, and legends."""
        app = QApplication.instance()
        if not app:
            return

        self._apply_qt_palette(app)
        self._apply_pg_globals()

        for pw in self._registry_plots:
            self._style_plot_widget(pw)

        for lgd in self._registry_legends:
            self._style_legend(lgd)

        self._apply_stylesheet(app)

    # ---------- Qt ----------

    def _apply_qt_palette(self, app):
        app.setPalette(self._dark_palette() if self.mode == self.DARK else QPalette())

    def _dark_palette(self):
        """Return a platform-optimized dark palette."""
        if sys.platform.startswith("win"):  # Windows
            return self._dark_palette_windows()
        else:  # Linux / macOS / fallback
            return self._dark_palette_unix()

    def _dark_palette_windows(self):
        p = QPalette()

        base_color = QColor(45, 45, 50)
        alt_color = QColor(35, 35, 40)
        text_color = QColor(230, 230, 230)

        p.setColor(QPalette.Window, base_color)
        p.setColor(QPalette.WindowText, text_color)

        p.setColor(QPalette.Base, alt_color)
        p.setColor(QPalette.AlternateBase, QColor(55, 55, 60))

        p.setColor(QPalette.Text, text_color)
        p.setColor(QPalette.Button, QColor(60, 60, 65))
        p.setColor(QPalette.ButtonText, text_color)

        p.setColor(QPalette.Highlight, QColor(100, 160, 220))
        p.setColor(QPalette.HighlightedText, QColor(20, 20, 20))

        p.setColor(QPalette.ToolTipBase, QColor(65, 65, 70))
        p.setColor(QPalette.ToolTipText, QColor(245, 245, 245))

        p.setColor(QPalette.Light, QColor(70, 70, 75))
        p.setColor(QPalette.Midlight, QColor(60, 60, 65))
        p.setColor(QPalette.Dark, QColor(30, 30, 35))
        p.setColor(QPalette.Shadow, QColor(20, 20, 25))

        p.setColor(QPalette.Link, QColor(100, 160, 220))
        p.setColor(QPalette.LinkVisited, QColor(150, 120, 200))

        return p

    def _dark_palette_unix(self):
        p = QPalette()

        # --- Active state (normal widgets) ---
        p.setColor(QPalette.Window, QColor(40, 40, 45))
        p.setColor(QPalette.WindowText, QColor(220, 220, 220))
        p.setColor(QPalette.Base, QColor(30, 30, 35))
        p.setColor(QPalette.AlternateBase, QColor(50, 50, 55))
        p.setColor(QPalette.Text, QColor(220, 220, 220))
        p.setColor(QPalette.Button, QColor(55, 55, 60))
        p.setColor(QPalette.ButtonText, QColor(230, 230, 230))
        p.setColor(QPalette.Highlight, QColor(100, 160, 220))
        p.setColor(QPalette.HighlightedText, QColor(20, 20, 20))
        p.setColor(QPalette.ToolTipBase, QColor(60, 60, 65))
        p.setColor(QPalette.ToolTipText, QColor(240, 240, 240))

        # --- Disabled state ---
        p.setColor(QPalette.Disabled, QPalette.WindowText, QColor(140, 140, 145))
        p.setColor(QPalette.Disabled, QPalette.Text, QColor(150, 150, 155))
        p.setColor(QPalette.Disabled, QPalette.ButtonText, QColor(140, 140, 145))
        p.setColor(QPalette.PlaceholderText, QColor(130, 130, 135))

        # Slightly lighter than active Base so fields remain visible
        p.setColor(QPalette.Disabled, QPalette.Base, QColor(45, 45, 50))

        # Keep buttons visually recessed
        p.setColor(QPalette.Disabled, QPalette.Button, QColor(48, 48, 54))

        # Muted blue-gray highlight
        p.setColor(QPalette.Disabled, QPalette.Highlight, QColor(70, 80, 95))
        p.setColor(QPalette.Disabled, QPalette.HighlightedText, QColor(170, 170, 175))

        return p

    def _apply_stylesheet(self, app):
        if self.mode == self.DARK:
            app.setStyleSheet("""
            QCheckBox::indicator {
                width: 10px;
                height: 10px;
                border: 1px solid #888;
                background: #222;
            }

            QCheckBox::indicator:checked {
                background: solid #b3b3b3;
            }
            

            QLineEdit::placeholder {
                color: #dddddd;
            }
            
            QMenu::separator {
                height: 0.5px;
                background: palette(mid);
                margin-left: 8px;
                margin-right: 8px;
            }
            """)
        else:
            app.setStyleSheet("")

    # ---------- pyqtgraph ----------

    def _apply_pg_globals(self):
        if self.mode == self.DARK:
            pg.setConfigOption("background", (30, 30, 30))
            pg.setConfigOption("foreground", "w")
        else:
            pg.setConfigOption("background", "w")
            pg.setConfigOption("foreground", "k")

    def _style_plot_widget(self, pw):
        try:
            pw.setBackground((30, 30, 30) if self.mode == self.DARK else "w")
        except AttributeError:
            pw.getViewBox().setBackgroundColor((30, 30, 30) if self.mode == self.DARK else "w")
            pw.getViewWidget().setBackgroundBrush(
                pg.mkBrush((30, 30, 30) if self.mode == self.DARK else "w")
            )
        
        except RuntimeError:
            return

        axis_pen = pg.mkPen("w" if self.mode == self.DARK else "k")
        for axis in ("left", "bottom", "right", "top"):
            ax = pw.getAxis(axis)
            if ax:
                ax.setPen(axis_pen)
                ax.setTextPen(axis_pen)

        pw.showGrid(x=True, y=True, alpha=0.2 if self.mode == self.DARK else 0.3)
        pw.enableAutoRange()

    def _style_legend(self, legend: pg.LegendItem):
        bg_color = (
            QColor(30, 30, 30) if self.mode == self.DARK else QColor(255, 255, 255)
        )
        bg_color.setAlpha(180 if self.mode == self.DARK else 220)
        legend.setBrush(pg.mkBrush(bg_color))

        fg_color = "w" if self.mode == self.DARK else "k"
        for sample, label in legend.items:
            label.setText(label.text, color=fg_color)
            if hasattr(sample, "setPen"):
                sample.setPen(pg.mkPen(fg_color))
            if hasattr(sample, "setBrush"):
                sample.setBrush(pg.mkBrush(fg_color))

    def style_hist_lut(self, hist):
        fg = "w" if self.mode == self.DARK else "k"

        # 1. Axis ticks + labels
        hist.axis.setPen(pg.mkPen(fg))
        hist.axis.setTextPen(pg.mkPen(fg))

        # 2. Histogram plot (inside the LUT widget)

        # 3. Gradient ticks (important)
        hist.gradient.tickPen = pg.mkPen(fg)
        hist.gradient.textPen = pg.mkPen(fg)

        # Force redraw
        hist.gradient.update()
        
