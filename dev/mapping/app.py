import sys
from pathlib import Path

from PySide6.QtWidgets import QApplication
from PySide6.QtWebEngineWidgets import QWebEngineView
from PySide6.QtCore import QUrl

from PySide6.QtWebEngineCore import QWebEngineSettings



app = QApplication(sys.argv)

view = QWebEngineView()
view.settings().setAttribute(
    QWebEngineSettings.WebAttribute.JavascriptEnabled, True
)
view.settings().setAttribute(
    QWebEngineSettings.WebAttribute.LocalContentCanAccessRemoteUrls, True
)
view.settings().setAttribute(
    QWebEngineSettings.WebAttribute.LocalContentCanAccessFileUrls, True
)
html_file = Path(__file__).parent / "map.html"

view.load(QUrl.fromLocalFile(str(html_file.resolve())))

view.resize(1200, 800)
view.show()

sys.exit(app.exec())