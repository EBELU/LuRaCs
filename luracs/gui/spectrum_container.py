from PySide6.QtWidgets import QWidget, QVBoxLayout, QStackedWidget, QTabWidget
from .SpectrumPlot import SpectrumPlot

class SpectrumContainer(QWidget):
    def __init__(self):
        super().__init__()

        self.stack = QStackedWidget()

        # --- Single plot mode ---
        self.single_plot = SpectrumPlot()
        self.single_page = QWidget()
        layout = QVBoxLayout(self.single_page)
        layout.addWidget(self.single_plot)

        # --- Multi plot mode ---
        self.tabs = QTabWidget()
        self.multi_page = QWidget()
        layout2 = QVBoxLayout(self.multi_page)
        layout2.addWidget(self.tabs)

        # Add to stack
        self.stack.addWidget(self.single_page)
        self.stack.addWidget(self.multi_page)

        main_layout = QVBoxLayout(self)
        main_layout.addWidget(self.stack)
        
    def set_single_mode(self):
        self.stack.setCurrentWidget(self.single_page)

    def set_multi_mode(self):
        self.stack.setCurrentWidget(self.multi_page)
        
    def remove_tab(self):
        index = self.tabs.indexOf(widget)
        self.tabs.removeTab(index)
        widget.deleteLater()