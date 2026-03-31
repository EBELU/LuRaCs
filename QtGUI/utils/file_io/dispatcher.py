from pathlib import Path
from .xml_parser import xml_parser

def io_dispatcher(file_name: Path | str):
    if isinstance(file_name, Path):
        pass
    elif isinstance(file_name, str):
        file_name = Path(file_name)
    else:
        raise ValueError(f"Unknown path type {type(file_name)}")
    
    if file_name.suffix in (".xml", ".n42"):
        try: 
            parsed = xml_parser(file_name)
        except Exception as e:
            print("Failed to parse XML file {file_name}, exeption raised {e}")
            return
        
        return parsed.kwargs