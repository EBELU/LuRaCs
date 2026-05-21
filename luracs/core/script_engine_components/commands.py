from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from clients.DeviceWrappers import WrappedRealTimePackage, WrappedStatusPackage
    from core.script_engine import ScriptEngine
    from .registry import CommandRegistry
    
from abc import ABC, abstractmethod
import asyncio
from pathlib import Path

from datetime import timedelta, datetime
from glob import glob

from utils.file_io import xml_parser, db_parser
from core import RunManager, SpectrumManager, Settings
from .helpers import print_table, TableFormatter, plot_spectrum, print_rois
from .exceptions import ArgumentError, ActiveGUIError, InvalidCommandError




class Command(ABC):
    name: str = ""
    aliases: list[str] = []
    argument_tree: dict = {}

    @abstractmethod
    async def run(self, engine, *args):
        pass
    
    @property
    def help(self) -> str:
        return self.run.__doc__ or "No help available."

    
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
    argument_tree = {"devices": None, "rois": None, "spectra": None, "spectrogram": None}
    async def run(self, engine, *args):
        """
        List available resources.

        Usage:
            list devices
            list rois
            list spectra
            list spectrogram

        Arguments:
            devices         Show connected devices
            rois            Show defined ROIs
            spectra         Show loaded spectra
            spectrogram     Show loaded spectrogram
        """ 
        if not args:
            return self.help

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
        
        elif args[0] == "spectrogram":
            spectrum_table = TableFormatter(["Index", "Name"], title = "Loaded Spectrogram")
            for i, key in enumerate(RunManager.loaded_spectrogram):
                spectrum_table.add_row([i, key])
            return spectrum_table.get_table()
        
        else:
            raise ArgumentError(f"{args[0]} is not a valid argument!")
        
class IndexCommand(Command):
    name = "index"
    argument_tree = {"spectrum": None, "spectrogram": None, "rois": None}
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
            table = TableFormatter(["Name", "Bounds", "Nuclides"], title="ROI Index")
            for file in glob(str(Settings.Paths.roi_library / "*.xml")):
                parser = xml_parser(file)
                rois = parser.get_rois()   
                table.add_row(
                    [Path(file).stem, 
                     ", ".join([str(i.roi_bound) for i in rois]),
                     ", ".join([str(i.emission.parent_nuclide) for i in rois])
                    ])
                
            return table.get_table()
            
            
        
class ViewCommand(Command):
    name = "view"
    argument_tree = {"spectrum": None, "rois": None, "log": None}
    async def run(self, engine, *args):
        if not engine.headless:
            raise ActiveGUIError("view")
        if len(args) == 0:
            raise ArgumentError("View must be followed by 'spectrum', 'device' or 'log'")
        
        if args[0] == "spectrum":
            plot_spectrum(args[1:])
        
        elif args[0] == "rois":
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