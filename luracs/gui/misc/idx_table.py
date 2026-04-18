from PySide6.QtWidgets import (
    QWidget,
    QTableWidget,
    QTableWidgetItem,
    QAbstractItemView,
    QPushButton,
)


class StrIdxTable(QWidget):
    def __init__(
        self,
        title="",
        parent=None,
        columns=None,
        column_widths=None,
        has_menu_button=False,
    ):
        """Abstraction of QTable the that uses string keys for table indexing."""
        super().__init__(parent)

        self.table: QTableWidget = QTableWidget()
        self.table.setEditTriggers(QAbstractItemView.NoEditTriggers)

        self.has_been_set = False

        self.current_keys = set([])

        self.has_menu_button = has_menu_button

        if columns is not None:
            self.reset_table(columns, column_widths)

    def get_key_from_index(self, index: int) -> str:
        item = self.table.item(index, 0)
        return item.text() if item else None

    def get_index_from_key(self, key: str) -> int | None:
        key = str(key)
        for row in range(self.table.rowCount()):
            item = self.table.item(row, 0)
            if item is not None and item.text() == key:
                return row
        return None

    def get_all_keys(self) -> set[str]:
        keys = []
        for row in range(self.table.rowCount()):
            item = self.table.item(row, 0)
            if item is not None:
                keys.append(item.text())
        return set(keys)

    def reset_table(self, titles: list, widths=None):
        "Clear the table and set new columns titles"
        assert isinstance(titles, list), f"Titles must be a list! Is {type(titles)}"
        if self.table is not None:
            self.table.clear()

        self.table.setRowCount(0)
        titles = [""] + titles  # Create columns for key index storage

        if self.has_menu_button:
            titles = [""] + titles
            self.table.setColumnCount(len(titles))
            self.table.setHorizontalHeaderLabels(titles)
        else:
            self.table.setColumnCount(len(titles))
            self.table.setHorizontalHeaderLabels(titles)
        self.table.setMinimumHeight(50)

        if widths is not None:
            if self.has_menu_button:
                assert len(widths) == len(titles) - 1, (
                    f"Length of widths does not match, titles {len(titles)}, widths {len(widths)}"
                )
                for i in range(len(widths)):
                    self.table.setColumnWidth(i + 1, widths[i])
            else:
                assert len(widths) == len(titles) - 1, (
                    f"Length of widths does not match, titles {len(titles)}, widths {len(widths)}"
                )
                for i in range(len(widths)):
                    self.table.setColumnWidth(i + 1, widths[i])

        self.table.setColumnHidden(0, True)

    def write_row(
        self,
        row_tag: str,
        values: list,
        menu_button: QWidget = None,
        force__button_overwrite=False,
    ):
        assert isinstance(values, list), f"Values must be a list! Is {type(values)}"

        self.table.setSortingEnabled(False)
        try:
            row_index = self.get_index_from_key(row_tag)
            if row_index is None:
                row_index = self.table.rowCount()
                self.table.insertRow(row_index)
                self.table.setItem(row_index, 0, QTableWidgetItem(str(row_tag)))

            # Column layout
            data_start_col = 2 if self.has_menu_button else 1

            # Check if button already exists
            existing_button = None
            if self.has_menu_button:
                existing_button = self.table.cellWidget(row_index, 1)

            # Only set button if none exists or is forced to overwrite
            if (
                self.has_menu_button
                and menu_button is not None
                and existing_button is None
            ) or (force__button_overwrite and menu_button is not None):
                self.table.setCellWidget(row_index, 1, menu_button)

            # Data cells
            for col_index, value in enumerate(values):
                self.table.setItem(
                    row_index, data_start_col + col_index, QTableWidgetItem(str(value))
                )

        finally:
            self.table.setSortingEnabled(True)

    def delete_row(self, row_tag: str):
        row_index = self.get_index_from_key(row_tag)
        if row_index is None:
            return
        self.table.removeRow(row_index)
