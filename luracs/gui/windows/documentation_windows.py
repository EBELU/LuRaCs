from PySide6.QtWidgets import QTextBrowser, QListView, QDialog, QHBoxLayout, QVBoxLayout, QFileSystemModel, QTreeView, QWidget, QPushButton
from PySide6.QtCore import QSortFilterProxyModel, Qt, QUrl

import markdown
from pathlib import Path




class SmallDocumentationDialog(QDialog):
    def __init__(self, md_file: str, parent=None):
        super().__init__(parent)

        self.setWindowTitle("Documentation")

        self.resize(800, 600)

        main_layout = QVBoxLayout(self)

        self.text_browser = QTextBrowser()
        self.text_browser.setOpenExternalLinks(True)

        main_layout.addWidget(self.text_browser)

        # Bottom button row
        button_layout = QHBoxLayout()
        button_layout.addStretch()

        close_button = QPushButton("Close")
        close_button.clicked.connect(self.accept)

        button_layout.addWidget(close_button)

        main_layout.addLayout(button_layout)

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

        # Main vertical layout
        main_layout = QVBoxLayout(self)

        # Content area
        content_layout = QHBoxLayout()

        self.doc_list = QListView()
        self.text_browser = QTextBrowser()
        self.text_browser.setOpenExternalLinks(True)

        self.model = QFileSystemModel()
        self.model.setRootPath(str(doc_dir))

        self.model.setNameFilters(["*.md"])
        self.model.setNameFilterDisables(False)

        self.doc_tree = QTreeView()
        self.doc_tree.setMaximumWidth(350)

        content_layout.addWidget(self.doc_tree, 3)
        content_layout.addWidget(self.text_browser, 7)

        main_layout.addLayout(content_layout)

        self.doc_tree.setModel(self.model)
        self.doc_tree.setRootIndex(self.model.index(str(doc_dir)))

        self.doc_tree.selectionModel().selectionChanged.connect(
            self.on_selection_changed
        )

        self.model.directoryLoaded.connect(self.on_directory_loaded)

        for i in range(1, self.model.columnCount()):
            self.doc_tree.hideColumn(i)

        # Bottom button row
        button_layout = QHBoxLayout()
        button_layout.addStretch()

        close_button = QPushButton("Close")
        close_button.clicked.connect(self.close)

        button_layout.addWidget(close_button)

        main_layout.addLayout(button_layout)

        self.load_markdown(doc_dir / "Welcome.md")
        
        
    def load_markdown(self, path: str | Path):
        path = Path(path)

        md_text = path.read_text(encoding="utf-8")

        html = markdown.markdown(
            md_text,
            extensions=["fenced_code", "tables",         "sane_lists"]
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
        img {{
            max-width: 100%;
            height: auto;
            display: block;
            margin: 10px auto;
            cursor: zoom-in;
        }}
        h1 {{ color: #1a73e8; }}
        th, td {{
            padding: 2px 2px;
            text-align: left;
        }}

        th {{
            font-weight: bold;
        }}

        p {{
            margin-top: 1px;
            margin-bottom: 1px;
        }}
        
        ul, ol {{
            margin-top: 4px;
            margin-bottom: 4px;
            padding-left: 24px;
        }}

        li p {{
            margin: 0;
        }}
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
            styled_html,
        )
        self.text_browser.document().setBaseUrl(
            QUrl.fromLocalFile(str(path.parent) + "/")
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