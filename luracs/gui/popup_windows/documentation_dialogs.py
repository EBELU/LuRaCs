from PySide6.QtWidgets import QTextBrowser, QListView, QDialog, QHBoxLayout, QFileSystemModel, QTreeView, QWidget
from PySide6.QtCore import QUrl, Qt
from PySide6.QtGui import QFont, QStandardItem, QStandardItemModel
import markdown
from pathlib import Path
from glob import glob

class SmallDocumentationDialog(QDialog):
    def __init__(self, md_file: str, parent=None):
        super().__init__(parent)

        self.setWindowTitle("Documentation")
        
        width = 600
        height = int(width * 1.414)
        self.resize(width, height)
        main_layout = QHBoxLayout(self)
        
        self.text_browser = QTextBrowser()
        self.text_browser.document().setDefaultFont(QFont("Arial", 12))
        
        main_layout.addWidget(self.text_browser)
        
        self.load_markdown(md_file)
        
    def load_markdown(self, path: str | Path):
        path = Path(path)

        md_text = path.read_text(encoding="utf-8")

        html = markdown.markdown(
            md_text,
            extensions=["fenced_code", "tables"]
        )

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
        self.text_browser.setHtml(
            styled_html
        )


class DocumentationDialog(QWidget):
    def __init__(self, doc_dir: str, parent=None):
        super().__init__(parent)
        doc_dir = Path(doc_dir)

        self.setWindowTitle("Documentation")
        self.resize(800, 600)
        main_layout = QHBoxLayout(self)
        
        self.doc_list = QListView()
        self.text_browser = QTextBrowser()
        


        
        self.model = QFileSystemModel()
        self.model.setRootPath(str(doc_dir))

        # Only show markdown files
        self.model.setNameFilters(["*.md"])
        self.model.setNameFilterDisables(False)
        
        self.doc_tree = QTreeView()
        main_layout.addWidget(self.doc_tree, 3)
        main_layout.addWidget(self.text_browser, 7)

        self.doc_tree.setModel(self.model)
        self.doc_tree.setRootIndex(self.model.index(str(doc_dir)))

        self.doc_tree.selectionModel().selectionChanged.connect(self.on_selection_changed)
        
        self.model.directoryLoaded.connect(self.on_directory_loaded)
        for i in range(1, self.model.columnCount()):
            self.doc_tree.hideColumn(i)
        self.load_markdown(doc_dir / "Welcome.md")
        
    def load_markdown(self, path: str | Path):
        path = Path(path)

        md_text = path.read_text(encoding="utf-8")

        html = markdown.markdown(
            md_text,
            extensions=["fenced_code", "tables"]
        )

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
        self.text_browser.setHtml(
            styled_html
        )
        
    def on_selection_changed(self, selected, deselected):
        indexes = selected.indexes()
        if not indexes:
            return

        index = indexes[0]

        # Ignore folders
        if self.model.isDir(index):
            return

        file_path = self.model.filePath(index)
        self.load_markdown(file_path)
        
    def on_directory_loaded(self, path):
        self.doc_tree.expandAll()