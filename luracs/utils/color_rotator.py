from enum import Enum

from PySide6.QtGui import QColor, QPen
import pyqtgraph as pg


def shift_hue(color: QColor, degrees: int) -> QColor:
    """
    Shift the hue of a QColor by the given number of degrees.

    Args:
        color: Input QColor
        degrees: Hue shift in degrees (can be negative)

    Returns:
        New QColor with shifted hue
    """
    if not color.isValid():
        return QColor()

    h, s, v, a = color.getHsv()

    # Grayscale colors have hue == -1
    if h == -1:
        return QColor(color)

    new_hue = (h + degrees) % 360

    shifted = QColor()
    shifted.setHsv(new_hue, s, v, a)
    return shifted

class ColorRotator:
    class ColorSchemes(Enum):
        CATPPUCCIN = "catppuccin"
        MPL = "mpl"
        LO = "lo"
    
    def __init__(self, colors: ColorSchemes = None, width = 2):
        if colors is None:
            colors = ColorRotator.ColorSchemes.MPL
        if colors == self.ColorSchemes.MPL:  # Matplotlib
            colors = [
                "#1f77b4",
                "#ff7f0e",
                "#2ca02c",
                "#d62728",
                "#9467bd",
                "#8c564b",
            ]
        elif colors == self.ColorSchemes.LO:  # LibreOffice
            colors = [
                "#004586",
                "#ff420e",
                "#ffd320",
                "#579d1c",
                "#7e0021",
                "#83caff",
            ]
            
        elif colors == self.ColorSchemes.CATPPUCCIN: # Catppuccin Late
            colors = [
                "#1e66f5",
                "#fe640b",
                "#40a02b",
                "#e64553",
                "#8839ef",
                "#179299",
                "#ea76cb",
            ]

        # Normalize everything to QColor
        self.colors = [QColor(c) for c in colors]

        self.width = width
        self._i = 0

    def next_color(self) -> QColor:
        "Get the next color in the rotation"
        color = self.colors[self._i % len(self.colors)]
        self._i += 1
        return QColor(color)  # return a copy (safe to modify)
    
    def next_pen(self) -> QPen:
        return pg.mkPen(self.next_color(), width=self.width)
    
    def get_color_pair(self, hue_shift_degrees: int = 15) -> tuple[QColor, QColor]:
        "Get the next color in the rotation and a second color based on the first with shifted hue"
        
        first_color = self.next_color()
        second_color = shift_hue(first_color, hue_shift_degrees)
        return first_color, second_color
    
    def reset(self):
        self._i = 0