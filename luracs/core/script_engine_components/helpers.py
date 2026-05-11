from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from clients.DeviceWrappers import WrappedRealTimePackage, WrappedStatusPackage
    from core.script_engine import ScriptEngine
    
import plotext as plt
import numpy as np
from typing import Any
import sys

from core import SpectrumManager, Settings
from .exceptions import ArgumentError, ActiveGUIError, InvalidCommandError



class ArgumentParser:
    def __init__(self, signatures: dict[str, Any], command: str):
        self.signatures = signatures
        self.command = command
        
    def parse(self, args: list[str]) -> tuple[dict[str, Any], list[Any]]:
        i = 0
        while i < len(args):
            arg_key = args[i]
            if arg_key.startswith("-"):
                if arg_key not in self.signatures:
                    raise ArgumentError(f"{arg_key} is not a valid argument for {self.command}, valid arguments are {self.signatures.keys()}")
                default_argument = self.signatures[arg_key]
                if isinstance(default_argument, (tuple, list)):
                    self.signatures[arg_key] = args[i:i+len(default_argument)]
                    assert not any([p.startswith("-") for p in self.signatures[args]])
                    i += (1 + len(default_argument))
                elif isinstance(default_argument, bool):
                    self.signatures[arg_key] = not default_argument
                    i += 1
                else:
                    self.signatures[arg_key] = args[i+1]
                    i += 2

            else:
                break
            
        return self.signatures, args[i:]
    
class TableFormatter:
    def __init__(self, col_titles: list, col_types: list | None = None, title: str | None = None):
        self.col_titles = col_titles
        self.types = col_types
        self.rows = []
        self.col_widths = [len(t) for t in col_titles]
        self.title = title
        
    def add_row(self, row: list):
        assert len(row) == len(self.col_titles)
        for i, element in enumerate(row):
            self.col_widths[i] = max(self.col_widths[i], len(str(element)))
        self.rows.append(row)
        
    def _get_title(self):
        middle = f"| {self.title} |\n"
        lines = "=" * (len(middle) - 1) +"\n"
        
        return lines + middle + lines + "\n"
        
    def get_table(self):
        # Header
        header = " | ".join(
            f"{title:<{self.col_widths[i]}}"
            for i, title in enumerate(self.col_titles)
        )

        # Separator
        separator = "-+-".join(
            "-" * self.col_widths[i]
            for i in range(len(self.col_titles))
        )

        # Rows
        rows = []
        for row in self.rows:
            formatted_row = " | ".join(
                f"{str(cell):<{self.col_widths[i]}}"
                for i, cell in enumerate(row)
            )
            rows.append(formatted_row)
            
        title = self._get_title() if self.title is not None else ""
            

        return title + "\n".join([header, separator] + rows) + "\n"
    

def plot_spectrum(args):
    signatures = {
        "-n": None,
        "-Emin": -np.inf,
        "-Emax": np.inf,
        "-cps": False,
        "-log": False,
        "-bkgSub": False,
        "-A": False
    }
    
    arg_parser = ArgumentParser(signatures, "'view spectrum'")
    kwargs, posargs = arg_parser.parse(args)

    if kwargs["-n"] is None and len(posargs) > 0:
        spectrum_name = posargs[0]
    else:
        spectrum_name = kwargs["-n"]
    
    if spectrum_name is None:
        raise ArgumentError("No argument given for view spectrum")
    
    if spectrum_name == "all" or kwargs["-A"]:
        
        plt.clear_figure()
        for name, spectrum in SpectrumManager.get_spectra_dict().items():
            fg = spectrum.get_foreground(kwargs["-log"], kwargs["-cps"]) if not kwargs["-bkgSub"] else spectrum.get_bkg_sub(kwargs["-log"])
    
            fg = fg[(float(kwargs["-Emin"]) <= spectrum.x_axis) & (spectrum.x_axis <= float(kwargs["-Emax"]))]
            plt.plot(spectrum.x_axis, fg, label = name)
        
    
    else:
        spectrum = SpectrumManager.get_spectrum(spectrum_name)
        

        if spectrum is None:
            try:
                
                idx = int(spectrum_name)
                keys = list(SpectrumManager.get_spectra_dict().keys())
                spectrum = SpectrumManager.get_spectrum(keys[idx])
                if spectrum_name is None:
                    return
            except ValueError:
                raise ArgumentError(f"{spectrum_name} is not a valid index")
            
            except IndexError:
                raise ArgumentError(f"Given index is out of range for spectrum list containing {len(SpectrumManager.get_spectra_dict())} item(s)")
        
        fg = spectrum.get_foreground(kwargs["-log"], kwargs["-cps"]) if not kwargs["-bkgSub"] else spectrum.get_bkg_sub(kwargs["-log"])
        
        fg = fg[(float(kwargs["-Emin"]) <= spectrum.x_axis) & (spectrum.x_axis <= float(kwargs["-Emax"]))]
        

        plt.clear_figure()
        plt.plot(spectrum.x_axis, fg, label = spectrum.name)
        
    plt.xlabel("Energy [keV]")
    plt.ylabel("Counts" if not kwargs["-cps"] else "CPS")
    plt.theme("default" if Settings.Appearance.theme.upper() == "LIGHT" else "pro")
    plt.show()

def print_rois(by="roi", cps=False):
    str_parts = []
    if by == "roi":
        for roi_tag in SpectrumManager.ROIManager.ROIs.keys():
            table = TableFormatter(
                ["Spectrum", "Lower", "Upper", "Centroid", "FWHM", "Peak Counts"],
                title=SpectrumManager.ROIManager.ROIs[roi_tag].alias
            )
            for spectrum_name, roi in SpectrumManager.ROIManager.get_data_from_roi(roi_tag).items():
                
                if roi.fit:
                    centroid = round(roi.fit.mu, 3)
                    fwhm = round(roi.fit.fwhm, 3)
                    peak_counts = round(roi.get_count_data("peak_counts", cps), 3)
                else:
                    centroid = fwhm = peak_counts = None

                table.add_row([
                    spectrum_name,
                    f"{round(roi.roi_bound[0])} keV",
                    f"{round(roi.roi_bound[1])} keV",
                    f"{centroid} keV" if centroid is not None else "None",
                    f"{fwhm} keV" if fwhm is not None else "None",
                    str(peak_counts) if peak_counts is not None else "None"
                ])

            str_parts.append(table.get_table())

        return "\n".join(str_parts)

last_lines = 0
def print_table(table_text):
    global last_lines

    lines = table_text.count("\n") + 1

    if last_lines > 0:
        sys.stdout.write(f"\033[{last_lines}A")  # go up
        sys.stdout.write("\033[J")               # clear

    print(table_text, end="", flush=True)

    last_lines = lines