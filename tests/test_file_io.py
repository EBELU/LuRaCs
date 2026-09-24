from luracs.core import IOManager, SpectrumManager
from luracs.utils import file_io
import pytest

pytestmark = pytest.mark.order(2)

def test_spe_import(test_data, qtbot):
    file = test_data / "fontan.Spe"
    
    IOManager.Importer.import_generic_paths(file, selected_filter=IOManager.Importer.import_filters["spectrum"])
    qtbot.wait(100)

def test_tka_import(test_data, qtbot):
    file = test_data / "HPGeEU152.TKA"
    
    IOManager.Importer.import_generic_paths(file, selected_filter=IOManager.Importer.import_filters["spectrum"])
    qtbot.wait(100)

def test_radiacode_import(test_data, qtbot):
    file = test_data / "103-GRF-Eu152.xml"
    
    IOManager.Importer.import_generic_paths(file, selected_filter=IOManager.Importer.import_filters["spectrum"])
    qtbot.wait(100)

def test_raysid_import(test_data, qtbot):
    file = test_data / "Raysid-GRF-Eu152.xml"
    
    IOManager.Importer.import_generic_paths(file, selected_filter=IOManager.Importer.import_filters["spectrum"])
    qtbot.wait(100)
    
def test_background_import(test_data, qtbot):
    file = test_data / "Raysid-GRF-Eu152.xml"
    qtbot.wait(100)
    IOManager.Importer.import_generic_paths(file, selected_filter=IOManager.Importer.import_filters["spectrum"])
    
    assert "Raysid-GRF-Eu152" in SpectrumManager.spectrum_registry
    
    spectrum_parser = file_io.io_dispatcher(file)
    if spectrum_parser is not None:
        IOManager.Importer.sigImportSpectrumAsBackground.emit("Raysid-GRF-Eu152", spectrum_parser.data)
        
def test_export_xml(tmp_path, qtbot):
    spect = SpectrumManager.get_spectrum("Raysid-GRF-Eu152")
    IOManager.Exporter.export_spectrum(spect, "XML/n42 (*xml)", tmp_path / "tmpSpectXML.xml")
    qtbot.wait(100)
    
def test_export_csv(tmp_path, qtbot):
    spect = SpectrumManager.get_spectrum("Raysid-GRF-Eu152")
    IOManager.Exporter.export_spectrum(spect, "CSV (*.csv)", tmp_path / "tmpSpectXML.csv")
    qtbot.wait(100)
    
def test_export_xlsx(tmp_path, qtbot):
    spect = SpectrumManager.get_spectrum("Raysid-GRF-Eu152")
    IOManager.Exporter.export_spectrum(spect, "Excel Workbook (*.xlsx)", tmp_path / "tmpSpectXML.xlsx")
    qtbot.wait(100)
    
def test_export_rois_xml(tmp_path, qtbot):
    spect = SpectrumManager.get_spectrum("Raysid-GRF-Eu152")
    IOManager.Exporter.export_roi(spect, "XML (*xml)", tmp_path / "tmpSpectXML.xlsx")
    qtbot.wait(100)
    
def test_close_files(qtbot):
    
    for spect in ["103-GRF-Eu152", "HPGeEU152.TKA", "fontan.Spe"]:
        
        assert spect in SpectrumManager.spectrum_registry
        SpectrumManager.remove_spectrum(spect)
        assert spect not in SpectrumManager.spectrum_registry
        qtbot.wait(100)