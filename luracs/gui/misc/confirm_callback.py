from PySide6.QtWidgets import QMessageBox
from typing import Callable

def ConfirmCallback(parent, text: str, callback: Callable, *args, **kwargs):

    reply = QMessageBox.question(
    parent,
    "Confirm",
    text,
    QMessageBox.Yes | QMessageBox.No,
    QMessageBox.No  # default button
    )
    
    if reply == QMessageBox.Yes:
        callback(*args, **kwargs)