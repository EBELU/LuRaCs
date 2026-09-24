from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pytestqt import qtbot

    from luracs.main import MainWindow

from PySide6.QtCore import Qt, QTimer
from PySide6.QtWidgets import QDialogButtonBox

from luracs.core import RunManager, SpectrumManager
from luracs.gui.dialogs.roi_editor import ROIEditor
import pytest

pytestmark = pytest.mark.order(1)

@pytest.mark.order(1)
def test_application_startup(app_context):
    app, window, script_engine = app_context

    assert app is not None
    assert window is not None
    assert script_engine is not None

@pytest.mark.order(2)
def test_main_window_shows(app_context, qtbot):
    app, window, script_engine = app_context

    window.show()
    qtbot.waitExposed(window)

    assert window.isVisible()


def test_add_detector(
    main_window: MainWindow,
    qtbot: qtbot,
):
    future = RunManager.add_device(
        "MockClent",
        "mock",
        "BLE",
    )

    future.result(timeout=5)

    assert "MockClient" in RunManager.device_registry
    qtbot.wait(500)
    
def test_spectrum_plot_interaction(
    main_window: MainWindow,
    qtbot: qtbot,
):
    main_window.main_menu_bar.combined_action.trigger()
    
    
    qtbot.mouseClick(
        main_window.spectrum_plot_container.single_plot.btn_cps,
        Qt.LeftButton,
    )
    qtbot.wait(50)
    
    qtbot.mouseClick(
        main_window.spectrum_plot_container.single_plot.btn_lin_log,
        Qt.LeftButton,
    )
    qtbot.wait(50)
    
    qtbot.mouseClick(
        main_window.spectrum_plot_container.single_plot.btn_lin_log,
        Qt.LeftButton,
    )
    qtbot.wait(50)
    
    qtbot.mouseClick(
        main_window.spectrum_plot_container.single_plot.btn_cursor,
        Qt.LeftButton,
    )
    qtbot.wait(50)
    
    qtbot.mouseClick(
        main_window.spectrum_plot_container.single_plot.btn_cursor,
        Qt.LeftButton,
    )
    
    combo = main_window.spectrum_plot_container.single_plot.cbox_bkg_choises

    for i in range(combo.count()):
        combo.setCurrentIndex(i)
        qtbot.wait(50)

        assert combo.currentIndex() == i
        
    combo.setCurrentIndex(0)
    
    main_window.main_menu_bar.tabbed_action.trigger()
    qtbot.wait(50)
    main_window.main_menu_bar.combined_action.trigger()


def test_roi_editor(
    main_window: MainWindow,
    qtbot: qtbot,
):
    main_window.main_menu_bar.combined_action.trigger()
    qtbot.mouseClick(
        main_window.spectrum_plot_container.single_plot.btn_mark_roi,
        Qt.LeftButton,
    )
    qtbot.wait(50)
    
    assert "ROI_0" in SpectrumManager.ROIManager.roi_registry
    
    roi = SpectrumManager.ROIManager.roi_registry["ROI_0"]
    
    editor = roi.roi_editor_dialog(
        roi.tag,
        roi.alias,
        *roi.getRegion(),
        fit_type=roi.fit_type,
        bkg_type=roi.bkg_type,
        bkg_est_channels=roi.bkg_est_channels,
        merge=roi.merge,
        poisson_weights=roi.poisson_weights,
        movable=roi.movable,
        emission=roi.emission,
        nuclide_lib_ref=roi.nuclide_lib_ref,
    )
    
    editor.show()
    
    assert editor.isVisible()
    qtbot.addWidget(editor)
    editor.show()

    # Initially Gaussian
    assert editor.fit_type.currentText() == "Gaussian"

    # Change to None
    editor.fit_type.setCurrentText("None")
    assert editor.fit_type.currentText() == "None"

    # Accept
    ok_button = editor.buttons.button(
        QDialogButtonBox.StandardButton.Ok
    )

    qtbot.mouseClick(ok_button, Qt.LeftButton)
    roi.update_self(**editor.get_values())
    
    assert roi.fit_type == "None"

    # Open another editor using the updated ROI state
    editor = ROIEditor(
        roi.tag,
        roi.alias,
        *roi.getRegion(),
        fit_type=roi.fit_type,
        bkg_type=roi.bkg_type,
        bkg_est_channels=roi.bkg_est_channels,
        merge=roi.merge,
        poisson_weights=roi.poisson_weights,
        movable=roi.movable,
        emission=roi.emission,
        nuclide_lib_ref=roi.nuclide_lib_ref,
    )

    qtbot.addWidget(editor)
    editor.show()

    assert editor.fit_type.currentText() == "None"

    # Change back to Gaussian
    editor.fit_type.setCurrentText("Gaussian")
    assert editor.fit_type.currentText() == "Gaussian"

    ok_button = editor.buttons.button(
        QDialogButtonBox.StandardButton.Ok
    )

    qtbot.mouseClick(ok_button, Qt.LeftButton)
    roi.update_self(**editor.get_values())

    assert roi.fit_type == "Gaussian"


    editor.fit_type.setCurrentText("None")

    assert not editor.merge.isEnabled()
    assert not editor.poisson_weights.isEnabled()
    assert not editor.bkg_type.isEnabled()
    assert not editor.merge.isChecked()

    editor.fit_type.setCurrentText("Gaussian")

    assert editor.merge.isEnabled()
    assert editor.poisson_weights.isEnabled()
    assert editor.bkg_type.isEnabled()


    editor = ROIEditor(
        roi.tag,
        roi.alias,
        *roi.getRegion(),
        fit_type=roi.fit_type,
        bkg_type=roi.bkg_type,
        bkg_est_channels=roi.bkg_est_channels,
        merge=roi.merge,
        poisson_weights=roi.poisson_weights,
        movable=roi.movable,
        emission=roi.emission,
        nuclide_lib_ref=roi.nuclide_lib_ref,
    )


    qtbot.addWidget(editor)

    # This runs while editor.exec() is running.
    QTimer.singleShot(
        0,
        lambda: qtbot.mouseClick(
            editor.delete_button,
            Qt.LeftButton,
        ),
    )

    res = editor.exec()

    assert res == ROIEditor.DELETE
    
    if res == roi.roi_editor_dialog.DELETE:
        roi.sigDeleteRequested.emit(roi.tag)
    
    assert not len(SpectrumManager.ROIManager.roi_registry)