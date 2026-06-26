from PySide6.QtWidgets import (
    QApplication,
    QTextBrowser,
    QGroupBox,
    QListView,
    QDialog,
    QHBoxLayout,
    QMainWindow,
)
from PySide6.QtCore import QUrl, Qt
from PySide6.QtGui import QFont
import markdown
from pathlib import Path

app = QApplication([])


class SmallDocumentationDialog(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)

        self.setWindowTitle("Documentation")

        width = 600
        height = int(width * 1.414)
        self.resize(width, height)
        main_layout = QHBoxLayout(self)

        self.text_browser = QTextBrowser()
        self.text_browser.document().setDefaultFont(QFont("Arial", 12))

        main_layout.addWidget(self.text_browser)

        self.load_markdown("debug/widgets/ex_doc.md")

    def load_markdown(self, path: str | Path):
        path = Path(path)

        md_text = path.read_text(encoding="utf-8")

        html = markdown.markdown(md_text, extensions=["fenced_code", "tables"])

        styled_html = f"""
        <html>
        <head>
        <style>
        body {{
            font-family: Arial;
            line-height: 1.3;
            padding: 12px;
        }}
        h1 {{ color: #2A6099; }}
        </style>
        </head>
        <body>
        {html}
        </body>
        </html>
        """

        # important for images + relative links
        base_path = path.parent.resolve()
        self.text_browser.setHtml(styled_html)


class DocumentationDialog(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)

        self.setWindowTitle("Documentation")
        self.resize(800, 600)
        main_layout = QHBoxLayout(self)

        self.doc_list = QListView()

        self.text_browser = QTextBrowser()

        main_layout.addWidget(self.doc_list, 3)
        main_layout.addWidget(self.text_browser, 7)

        self.load_markdown("debug/widgets/ex_doc.md")

    def load_markdown(self, path: str | Path):
        path = Path(path)

        md_text = path.read_text(encoding="utf-8")

        html = markdown.markdown(md_text, extensions=["fenced_code", "tables"])

        styled_html = f"""
        <html>
        <head>
        <style>
        body {{
            font-family: Arial;
            line-height: 1.3;
            padding: 12px;
        }}
        h1 {{ color: #1a73e8; }}
        </style>
        </head>
        <body>
        {html}
        </body>
        </html>
        """

        # important for images + relative links
        base_path = path.parent.resolve()
        self.text_browser.setHtml(styled_html)


md_text = "# Hello\n\nThis is **Markdown**."
html = markdown.markdown(md_text, extensions=["fenced_code"])

styled_html = f"""
<style>
body {{
    font-family: -apple-system, Segoe UI, Arial;
    line-height: 1.6;
    padding: 12px;
}}

h1, h2, h3 {{
    margin-top: 1em;
}}

pre {{
    background: #2b2b2b;
    color: #f8f8f2;
    padding: 10px;
    border-radius: 6px;
    overflow-x: auto;
}}

code {{
    font-family: Consolas, monospace;
}}

a {{
    color: #2980b9;
    text-decoration: none;
}}
</style>

{html}
</body>
</html>
"""

ml = """<html>
<head>
<style>
body {
    font-family: Arial;
    line-height: 1;
    padding: 12px;
    color: #2c3e50;
}

h1 {
    color: #1a73e8;
}

h2 {
    color: #34495e;
    margin-top: 1em;
}

p {
    margin: 8px 0;
}

b {
    color: #000;
}

i {
    color: #555;
}

a {
    color: #2980b9;
    text-decoration: none;
}
</style>
</head>
<body>

<h1>Hello</h1>

<p>This is <b>bold text</b> and this is <i>italic text</i>.</p>

<h2>Section</h2>

<p>
This is a paragraph with a 
<a href="https://example.com">link</a>.
</p>

<p>
Another paragraph to show spacing and readability.
</p>

</body>
</html>"""

# browser = QTextBrowser()
# browser.setHtml(ml)
# browser.show()

doc_dialog = SmallDocumentationDialog()
doc_dialog.show()

app.exec()
