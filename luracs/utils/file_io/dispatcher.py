from pathlib import Path
from .xml_parser import xml_parser
from .db_parser import db_parser


def io_dispatcher(
    file_name: Path | str, meta_parsing: bool = False
) -> xml_parser | db_parser:
    "Växeln, hallå hallå"
    if isinstance(file_name, Path):
        pass
    elif isinstance(file_name, str):
        file_name = Path(file_name)
    else:
        raise ValueError(f"Unknown path type {type(file_name)}")

    if not file_name.is_file():
        raise FileNotFoundError(f"File {file_name} could not be found at dispatch")

    if file_name.suffix in (".xml", ".n42"):
        return xml_parser(file_name, meta_parsing)

    elif file_name.suffix in (".tke", ".spe"):
        raise NotImplementedError()

    elif file_name.suffix in (".db"):
        return db_parser(file_name)
