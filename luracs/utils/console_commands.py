from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from clients.DeviceWrappers import WrappedRealTimePackage, WrappedStatusPackage
    from core.script_engine import ScriptEngine
    
from abc import ABC, abstractmethod
import asyncio
import plotext as plt
import numpy as np
from typing import Any
import sys
from pathlib import Path

from datetime import timedelta, datetime

from utils.file_io import xml_parser, db_parser
from core import RunManager, SpectrumManager, Settings

from glob import glob

class Command(ABC):
    name: str = ""
    aliases: list[str] = []

    @abstractmethod
    async def run(self, engine, *args):
        pass

class CommandRegistry:
    def __init__(self):
        self.commands = {}

    def register(self, command):
        self.commands[command.name] = command

    def get(self, name):
        return self.commands.get(name)
    
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
    
# --- Custom errors ---
class InvalidCommandError(Exception):
    def __init__(self, command, message="Invalid command"):
        self.command = command
        super().__init__(f"Command: {command} - {message}")
        
class ArgumentError(Exception):
    def __init__(self, command):
        super().__init__(str(command))
        
class ActiveGUIError(Exception):
    def __init__(self, command):
        super().__init__(f"{command} is only available when running in headless mode")


# --- Helpers ---
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
    
# ====== Commands ======
class ClearCommand(Command):
    name = "clear"
    async def run(self, engine, *args):
        color = engine.headless
        if color:
            bg_yellow = "\033[43m\033[30m"
            bg_blue   = "\033[44m"
            bg_green  = "\033[42m\033[30m"
            reset = "\033[0m"
        else:
            bg_yellow = ""
            bg_blue   = ""
            bg_green  = ""
            reset = ""
            

        default_message = (f"""{bg_green} ====== {reset}{bg_blue} ====== {reset}{bg_yellow} ====== {reset}
{bg_green}|71    |{reset}{bg_blue}|88    |{reset}{bg_yellow}|55    |{reset}    Version:  {engine.program_version}
{bg_green}|  Lu  |{reset}{bg_blue}|  Ra  |{reset}{bg_yellow}|  Cs  |{reset}    LuRaCs Console
{bg_green}| 177  |{reset}{bg_blue}| 226  |{reset}{bg_yellow}| 137  |{reset}    Type 'help' for a list of commands
{bg_green} ====== {reset}{bg_blue} ====== {reset}{bg_yellow} ====== {reset}
""")
        return default_message
    

class HelpCommand(Command):
    name = "help"
    async def run(self, engine, *args):
        help_text = (
            "Available commands:\n"
            "- help: Show this message\n"
            "- exit: Quit the application\n"
            "- clear: Clear the console and stop watching\n"
            "- list: \n"
            "   - list devices: List connected devices\n"
            "   - list spectra: List loaded spectra\n"
            "- scan: \n"
            "   - scan bt [device_name]: Scan for Bluetooth devices\n"
            "   - scan usb [device_name]: Scan for USB devices\n"
            "- view: \n"
            "   - view spectrum <spectrum_name | spectrum number>: Display a loaded spectrum\n"
            "   - view device <device_name | device number>: Display information about a connected device\n"
            "   - view logs [lines = 50]: Display recent log entries\n"
            # "- watch: \n"
            # "   - watch spectrum <spectrum_name | spectrum number>: Continuously display updates for a spectrum\n"
            # "   - watch device <device_name | device number>: Continuously display updates for a device\n"
            # "   - watch logs: Continuously display new log entries\n"
        )
        return help_text

class ListCommand(Command):
    name = "list"
    async def run(self, engine, *args):
        if not args:
            return "Usage: list [devices|spectra|spectrogram]"

        if args[0] == "devices":
            spectrum_table = TableFormatter(["Index", "Name"], title = "Connected Devices")
            for i, key in enumerate(RunManager.devices.keys()):
                spectrum_table.add_row([i, key])
            return spectrum_table.get_table()
        
        if args[0] == "rois":
            spectrum_table = TableFormatter(["Index", "Name"], title = "ROIs")
            for i, key in enumerate(SpectrumManager.ROIManager.ROIs.keys()):
                spectrum_table.add_row([i, key])
            return spectrum_table.get_table()
        
        elif args[0] == "spectra":            
            spectrum_table = TableFormatter(["Index", "Name"], title = "Loaded Spectra")
            for i, key in enumerate(SpectrumManager.get_spectra_dict().keys()):
                spectrum_table.add_row([i, key])
            return spectrum_table.get_table()
        
        else:
            return "Unknown list option. Use 'list devices' or 'list spectra'."
        
class IndexCommand(Command):
    name = "index"
    
    async def run(self, engine, *args):
        if len(args) == 0:
            raise ArgumentError("View must be followed by 'spectrum', 'spectrogram' or 'rois'")
        
        
        if args[0] in ("spectrum", "spectra"):
            table = TableFormatter(["Spectrum", "Date", "Duration", "Instrument"], title="Spectrum Index")
            for file in glob(str(Settings.Paths.spectrum_library / "*.xml")):
                parser = xml_parser(file, meta_only=True)
                header = parser.get_header()
                live_time = parser.get_foreground_spectrum().live_time
                if not live_time:
                    live_time = -1
                    
                table.add_row(
                    [header[0], 
                     parser.get_foreground_spectrum().start_date,
                     timedelta(seconds = round(live_time)),
                     header[2]
                     ])
        
            return table.get_table()
        
        elif args[0] in ("spectrogram", "sg"):
            table = TableFormatter(["Spectrogram", "Created", "Last Update", "Duration", "Dose [uSv]", "Instrument"], title="Spectrogram Index")
            for file in glob(str(Settings.Paths.spectrogram_library / "*.db")):
                parser = db_parser(file)
                header = parser.get_header()
                summary = parser.get_summary()
                    
                table.add_row(
                    [Path(file).name, 
                     header["created"],
                     summary["last_update"],
                     timedelta(seconds = round(summary["total_duration"])),
                     round(summary["total_dose"], 3),
                     header["device_id"]
                     ])
        
            return table.get_table()
        
        elif args[0] in ("roi", "rois"):
            pass
        
class ViewCommand(Command):
    name = "view"
    async def run(self, engine, *args):
        if not engine.headless:
            raise ActiveGUIError("view")
        if len(args) == 0:
            raise ArgumentError("View must be followed by 'spectrum', 'device' or 'log'")
        
        if args[0] == "spectrum":
            plot_spectrum(args[1:])
        
        elif args[0] == "roi":
            return print_rois()
        
        elif args[0] == "log":
            return "\n".join(list(engine.get_log_buffer()))
        
        else:
            raise InvalidCommandError(args[0])
        
        
class ROICommand(Command):
    name = "roi"
    async def run(self, engine, *args):
        if len(args) < 1:
            raise ArgumentError(f"Too few arguments")
        
        if "-c" in args:
            SpectrumManager.ROIManager.clear_all()
            return "All ROIs cleared"

        try:
            lower, upper = float(args[-2]), float(args[-1])
        except:
            raise ArgumentError("Arguments could not converted to float")
        
        SpectrumManager.ROIManager.add_roi(lower, upper)
        
        return f"ROI added"
    
class SpectrogramCommand(Command):
    name = "spectrogram"
    
    async def run(self, engine, *args):
        pass
    
class WatchCommand(Command):
    name = "watch"
    is_watching = False
    realtime_values_buffer: dict[str, WrappedRealTimePackage] = {}
    device_states_buffer: dict[str, WrappedStatusPackage] = {}
    
    def __init__(self):
        RunManager.currentUpdated.connect(self.update_realtime_values_buffer)
        RunManager.statusUpdated.connect(self.update_device_state_buffer)
        
    
    async def run(self, engine: ScriptEngine, *args):
        if not engine.headless:
            raise ActiveGUIError("watch")
        
        engine.sigCancelCurrent.connect(self.stop_watching)
        self.is_watching = True
        try:
            print_table("\033[2J\033[H")

            while self.is_watching:
                table = TableFormatter(["Device", "Count Rate [/s]", "Dose Rate [uSv/s]", "Battery [%]"])

                for device, realtime_values in self.realtime_values_buffer.items():
                    device_state = self.device_states_buffer.get(device)
                    if not device_state:
                        continue

                    table.add_row([
                        device,
                        round(realtime_values.CPS, 2),
                        round(realtime_values.DR, 3),
                        device_state.battery
                    ])

                print_table(table.get_table())

                await asyncio.sleep(Settings.Advanced.update_loop_delay)

        except asyncio.CancelledError:
            raise
        

    def update_realtime_values_buffer(self, device_name: str, data: WrappedRealTimePackage):
        self.realtime_values_buffer[device_name] = data

    def update_device_state_buffer(self, device_name: str, data: WrappedStatusPackage):
        self.device_states_buffer[device_name] = data

    def stop_watching(self):
        self.is_watching = False
        
        
class ExecCommand(Command):
    name = "exec"
    
    async def run(self, engine: ScriptEngine, *args):
        if len(args) != 1:
            raise ArgumentError("exec requires exactly one argument corresponding to a filepath")
        
        pth = Path.home() / Path(args[0])
        
        if not pth.is_file:
            raise ArgumentError(f"{pth} does not exists")
        
        engine.suppress_output(True)
        table = TableFormatter(["Lines executed"], title=f"Executed {pth.name}")

        with open(str(pth)) as f:
            for line in f.readlines():
                table.add_row([line])
                await engine.queue.put(line)
    
        return table.get_table()
        
# ===== Register commands ======
def register_commands(registry: CommandRegistry):
    registry.register(ClearCommand())
    registry.register(HelpCommand())
    registry.register(ListCommand())
    registry.register(ViewCommand())
    registry.register(ROICommand())
    registry.register(WatchCommand())
    registry.register(ExecCommand())
    registry.register(IndexCommand())