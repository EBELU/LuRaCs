import os
from PySide6.QtWidgets import (
    QDialog, QVBoxLayout, QListWidget, QListWidgetItem, QPushButton, QHBoxLayout
)
from PySide6.QtCore import Qt, Signal

class DataStore(QDialog):
    def __init__(self, title = "", parent = None):
        super().__init__(parent=parent)
        self.spectrogram_idx = []
        self.spectrum_idx = []
        self.roi_idx = []
        self.instrument_idx = []
        self.datalog_idx = []
        
    def run_indexing(self):
        pass