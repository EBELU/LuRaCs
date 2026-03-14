from lxml import etree
from uuid import uuid4
from datetime import datetime, timezone

NS = "http://physics.nist.gov/N42/2011/N42"
NSMAP = {None: NS}

def n42(tag):
    return f"{{{NS}}}{tag}"

def write_SpectrumData(data, root, kind):
    assert kind in ("Foreground", "Background")
    rad_measurement = etree.SubElement(root, n42("RadMeasurement"), id="Sample1")

    measurement_class = etree.SubElement(rad_measurement, n42("MeasurementClassCode"))
    measurement_class.text = "Background"

def write_xml_spectrum(spectrum = None):    
    root = etree.Element(
        n42("RadInstrumentData"),
        nsmap=NSMAP,
        n42DocUUID=str(uuid4()),
        n42DocDateTime=datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
    )
    
    creator = etree.SubElement(root, n42("RadInstrumentDataCreatorName"))
    creator.text = "MySpect"
    
    write_SpectrumData(None, root, "Foreground")
    
    tree = etree.ElementTree(root)
    tree.write(
        "example.n42",
        pretty_print=True,
        xml_declaration=True,
        encoding="utf-8"
    )
    
if __name__ == "__main__":
    write_xml_spectrum()