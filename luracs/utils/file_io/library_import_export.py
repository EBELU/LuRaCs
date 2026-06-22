from pathlib import Path
from zipfile import ZipFile, ZIP_DEFLATED


def zip_library(library_path: Path, output_file: Path) -> None:
    """
    Zip an entire library while preserving its directory structure.
    """
    library_path = library_path.resolve()

    with ZipFile(output_file, "w", compression=ZIP_DEFLATED) as zf:
        for file_path in library_path.rglob("*"):
            if file_path.is_file() and not file_path.name == "settings.json":
                # Store path relative to library root
                arcname = file_path.relative_to(library_path)
                zf.write(str(file_path), arcname)


def unzip_library(zip_file: Path, library_path: Path) -> None:
    """
    Restore the library into the target directory.
    """
    library_path.mkdir(parents=True, exist_ok=True)

    with ZipFile(zip_file, "r") as zf:
        zf.extractall(str(library_path))