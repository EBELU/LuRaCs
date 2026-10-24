
from PySide6.QtGui import Qt, QPainter, QPixmap

from luracs.core.settings import Settings


def create_splash_pixmap():
    """Create a splash screen image."""   
    image_pth = Settings.Paths.themes / "icons/splash_base.webp"
    
    pixmap = QPixmap(image_pth).scaled(
    800, 400,
    Qt.AspectRatioMode.KeepAspectRatio,
    Qt.TransformationMode.SmoothTransformation,
    )

    painter = QPainter(pixmap)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing)
    
    painter.end()
    return pixmap