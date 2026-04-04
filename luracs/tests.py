from utils.file_io import xml_parser, xml_writer, io_dispatcher
from SpectrumClasses import Spectrum

def test_xml_io():
    outpt = io_dispatcher(".appdata/roi_library/a.xml", meta_parsing=True)
    
    new_spect = Spectrum(len(outpt["foreground"].y_axis), outpt["name"])
    new_spect.set_foreground(outpt["foreground"])
    
    for r in outpt["peaks"]:
        new_spect.set_roi(r)
    
    xml_writer(new_spect, "/home/eewa/t", export_spectrum=False)
    print(io_dispatcher("/home/eewa/t.xml", meta_parsing=False))
if __name__ == "__main__":
    test_xml_io()