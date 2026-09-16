from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.core.script_engine import ScriptEngine

    from .registry import CommandRegistry


import asyncio
from abc import ABC, abstractmethod
from datetime import datetime, timedelta
from glob import glob
from pathlib import Path
from textwrap import dedent

from luracs.clients import ConnectionType, WrappedRealTimePackage, WrappedStatusPackage
from luracs.core import IOManager, RunManager, Settings, SpectrumManager
from luracs.utils import ascii_art
from luracs.utils.file_io import db_parser, xml_parser

from .exceptions import ActiveGUIError, ArgumentError, InvalidCommandError
from .helpers import (
    ArgumentParser,
    TableFormatter,
    plot_spectrum,
    print_rois,
    print_table,
)


class Command(ABC):
    name: str = ""
    aliases: list[str] = []

    @abstractmethod
    async def run(self, engine, *args):
        pass

    @property
    def help(self) -> str:
        return dedent(self.run.__doc__) or "No help available."

    def get_auto_complete(self) -> dict:
        return

    def index_files(self) -> None:
        return


class ClearCommand(Command):
    name = "clear"

    async def run(self, engine, *args):
        color = engine.headless

        return ascii_art.logo(engine.program_version, color, engine.IS_H3, "console") + "\n"


class HelpCommand(Command):
    name = "help"

    async def run(self, engine, *args):
        return """
Available commands
==================

General
-------
help
    Show this help message

clear
    Clear the console

exit | quit | shutdown
    Exit the application


Device Management
-----------------
device scan usb
    Scan for USB devices

device scan ble
    Scan for Bluetooth LE devices

device connect <device1> [device2 ...]
    Connect to one or more BLE devices

device disconnect <device>
    Disconnect a device

device disconnect all
    Disconnect all devices


Listing Resources
-----------------
list devices
    Show connected devices

list spectra
    Show loaded spectra

list spectrogram
    Show loaded spectrograms

list rois
    Show active ROIs


Indexes
-------
index spectrum
    Show indexed spectrum files

index spectrogram
    Show indexed spectrogram files

index roi
    Show indexed ROI files


Viewing Data
------------
view spectrum <name>
    Display a spectrum

view rois
    Display loaded ROIs

view log
    Display recent log entries


ROI Management
--------------
roi <lower> <upper>
    Add an ROI

roi -c
    Clear all ROIs


Spectrograms
------------
spectrogram start <device>
    Start a spectrogram

spectrogram start all
    Start spectrograms on all connected devices

Options:
    -n <name>     Custom name
    -i <seconds>  Save interval
    -c <count>    Concatenation factor

spectrogram pause <name>
spectrogram unpause <name>
spectrogram load <name>
spectrogram unload <name>


Monitoring
----------
watch
    Continuously display live device information
    Press Ctrl+C to stop


Automation
----------
exec <script_file>
    Execute a command script
"""


class ListCommand(Command):
    name = "list"

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
            spectrum_table = TableFormatter(
                ["Index", "Name"], title="Connected Devices"
            )
            for i, key in enumerate(RunManager.device_registry.keys()):
                spectrum_table.add_row([i, key])
            return spectrum_table.get_table()

        if args[0] == "rois":
            spectrum_table = TableFormatter(["Index", "Name"], title="ROIs")
            for i, key in enumerate(SpectrumManager.ROIManager.roi_registry.keys()):
                spectrum_table.add_row([i, key])
            return spectrum_table.get_table()

        elif args[0] == "spectra":
            spectrum_table = TableFormatter(["Index", "Name"], title="Loaded Spectra")
            for i, key in enumerate(SpectrumManager.get_spectra_dict().keys()):
                spectrum_table.add_row([i, key])
            return spectrum_table.get_table()

        elif args[0] == "spectrogram":
            spectrum_table = TableFormatter(
                ["Index", "Name"], title="Loaded Spectrogram"
            )
            for i, key in enumerate(RunManager.SpectrogramManager.spectrogram_registry):
                spectrum_table.add_row([i, key])
            return spectrum_table.get_table()

        else:
            raise ArgumentError(f"{args[0]} is not a valid argument!")

    def get_auto_complete(self):
        return {"devices": None, "rois": None, "spectra": None, "spectrogram": None}


class IndexCommand(Command):
    name = "index"

    async def run(self, engine, *args):
        if not args:
            return self.help
        if len(args) == 0:
            raise ArgumentError(
                "View must be followed by 'spectrum', 'spectrogram' or 'rois'"
            )

        if args[0] in ("spectrum", "spectra"):
            table = TableFormatter(
                ["Spectrum", "Date", "Duration", "Instrument"], title="Spectrum Index"
            )
            for file in glob(str(Settings.Paths.spectrum_library / "*.xml")):
                parser = xml_parser(file, meta_only=True)
                header = parser.get_header()
                live_time = parser.get_foreground_spectrum().live_time
                if not live_time:
                    live_time = -1

                table.add_row(
                    [
                        header.name,
                        parser.get_foreground_spectrum().start_date,
                        timedelta(seconds=round(live_time)),
                        header.instrument_id,
                    ]
                )

            return table.get_table()

        elif args[0] in ("spectrogram", "sg"):
            table = TableFormatter(
                [
                    "Spectrogram",
                    "Created",
                    "Last Update",
                    "Duration",
                    "Dose [uSv]",
                    "Instrument",
                ],
                title="Spectrogram Index",
            )
            for file in glob(str(Settings.Paths.spectrogram_library / "*.db")):
                parser = db_parser(file)
                header = parser.get_header()
                summary = parser.get_summary()

                table.add_row(
                    [
                        Path(file).name,
                        header.created,
                        summary.last_update,
                        timedelta(seconds=round(summary.total_duration)),
                        round(summary.total_dose, 3),
                        header.device_id,
                    ]
                )

            return table.get_table()

        elif args[0] in ("roi", "rois"):
            table = TableFormatter(["Name", "Bounds", "Nuclides"], title="ROI Index")
            for file in glob(str(Settings.Paths.roi_library / "*.xml")):
                parser = xml_parser(file)
                rois = parser.get_rois()
                table.add_row(
                    [
                        Path(file).stem,
                        ", ".join([str(i.roi_bound) for i in rois]),
                        ", ".join([str(i.emission.parent_nuclide) for i in rois]),
                    ]
                )

            return table.get_table()

    def get_auto_complete(self):
        return {"spectrum": None, "spectrogram": None, "rois": None}


class ViewCommand(Command):
    name = "view"

    async def run(self, engine, *args):
        if not engine.headless:
            raise ActiveGUIError("view")
        if len(args) == 0:
            raise ArgumentError(
                "View must be followed by 'spectrum', 'device' or 'log'"
            )

        if args[0] == "spectrum":
            plot_spectrum(args[1:])

        elif args[0] == "rois":
            return print_rois()

        elif args[0] == "log":
            return (
                "==============\n"
                + "| System Log |\n"
                + "==============\n\n"
                + "\n".join(list(engine.get_log_buffer()))
            )

        else:
            raise InvalidCommandError(args[0])

    def get_auto_complete(self):
        return {
            "spectrum": {
                key: None for key in SpectrumManager.get_spectra_dict().keys()
            },
            "rois": None,
            "log": None,
        }


class ROICommand(Command):
    name = "roi"
    index = []

    async def run(self, engine, *args):
        if len(args) < 1:
            raise ArgumentError("Too few arguments")

        if "-c" in args:
            SpectrumManager.ROIManager.clear_all()
            return "All ROIs cleared"

        try:
            lower, upper = float(args[-2]), float(args[-1])
        except ValueError:
            raise ArgumentError("Arguments could not converted to float")

        SpectrumManager.ROIManager.add_roi(lower, upper)

        return "ROI added"


class SpectrogramCommand(Command):
    name = "spectrogram"

    async def run(self, engine: ScriptEngine, *args):
        """
        Manage spectrogram acquisition and loaded spectrograms.

        Usage:
            spectrogram start [-n <name>] [-i <save_interval>] [-c <concat>] <device|all>
            spectrogram pause <name>
            spectrogram unpause <name>
            spectrogram load <path>
            spectrogram unload <name>

        Arguments:
            start           Start a new spectrogram for a device or for all devices
            pause           Pause a running spectrogram
            unpause         Resume a paused spectrogram
            load            Load a spectrogram from data store
            unload          Close a loaded spectrogram

        Options:
            -n <name>       Specify the name of the spectrogram
            -i <seconds>    Set the save interval (default: 1)
            -c <count>      Number of spectra to concatenate before saving (default: 1)

        Operands:
            <device>        Name of the device to acquire from
            all             Start a spectrogram for all connected devices
            <name>          Name of a loaded spectrogram
            <path>          Path to a saved spectrogram
        """
        if not args:
            return self.help
        
        if len(args) < 2:
            raise ArgumentError("Too few arguments")

        # --- Start a spectrogram ---
        if args[0] == "start":
            arg_parser = ArgumentParser({"-n": None, "-i": 1, "-c": 1}, self.name)
            parsed_args, pos_args = arg_parser.parse(args[1:])

            if args[-1] == "all":
                if parsed_args["-n"] is not None:
                    raise ArgumentError("Name can not be specified when mass starting")

                for device in RunManager.device_registry:
                    engine.thread_bridge.start_spectrogram(
                        f"Spectrogram_{device}_{datetime.now().replace(microsecond=0).isoformat()}",
                        device,
                        int(parsed_args["-i"]),
                        int(parsed_args["-c"]),
                    )
            else:
                engine.thread_bridge.start_spectrogram(
                    f"Spectrogram_{pos_args[0]}_{datetime.now().replace(microsecond=0).isoformat()}"
                    if parsed_args["-n"] is None
                    else parsed_args["-n"],
                    pos_args[0],
                    int(parsed_args["-i"]),
                    int(parsed_args["-c"]),
                )

            return

        # --- Pause a spectrogram ---
        elif args[0] == "pause":
            if args[1] not in RunManager.SpectrogramManager.spectrogram_registry:
                raise ArgumentError(f"'{args[1]}' does not match a loaded spectrogram")

            sg = RunManager.SpectrogramManager.spectrogram_registry[args[1]]
            if sg.paused:
                return f"'{args[1]}' is already paused"
            else:
                sg.pause_unpause()
                return f"'{args[1]}' paused"

        # --- Unpause a spectrum ---
        elif args[0] == "unpause":
            if args[1] not in RunManager.SpectrogramManager.spectrogram_registry:
                raise ArgumentError(f"'{args[1]}' does not match a loaded spectrogram")

            sg = RunManager.SpectrogramManager.spectrogram_registry[args[1]]
            if not sg.paused:
                return f"'{args[1]}' is already running"
            else:
                sg.pause_unpause()
                return f"'{args[1]}' unpaused"

        # --- Load a spectrogram form index ---
        elif args[0] == "load":
            file_pth = Path(args[1])
            if not file_pth.exists():
                raise ArgumentError(
                    f"'{args[1]}' does not match an existing spectrogram"
                )

            engine.thread_bridge.restart_spectrogram(file_pth.stem)
            return f"'{file_pth.stem}' loaded"

        # --- Unload a spectrogram from active ---
        elif args[0] == "unload":
            if args[1] not in RunManager.SpectrogramManager.spectrogram_registry:
                raise ArgumentError(f"'{args[1]}' does not match a loaded spectrogram")

            engine.thread_bridge.close_spectrogram(args[1])
            return f"'{args[1]}' closed"

        else:
            raise InvalidCommandError(args[0])

    def get_auto_complete(self):
        return {
            "start": {"all": None}
            | {key: None for key in RunManager.device_registry.keys()},
            "pause": {"all": None}
            | {
                key: None
                for key, sg in RunManager.SpectrogramManager.spectrogram_registry.items()
                if not sg.paused
            },
            "unpause": {"all": None}
            | {
                key: None
                for key, sg in RunManager.SpectrogramManager.spectrogram_registry.items()
                if sg.paused
            },
            "load": {
                file: None
                for file in IOManager.FileIndex.spectrogram_index.index_registry
            },
            "unload": {"all": None}
            | {key: None for key in RunManager.SpectrogramManager.spectrogram_registry.keys()},
        }

class MapCommand(Command):
    name = "map"
    
    async def run(self, engine: ScriptEngine, *args):
        """
        Load a map to the map window.

        Usage:
            map url <https-URL>
            map file <path to .pmtiles>

        Arguments:
            url             Load an online map from the specified HTTPS URL
            file            Load a local offline map from the specified .pmtiles file
        """
        if not args:
            return self.help
            
        if len(args) != 2:
            raise ArgumentError("The 'map'-command takes 2 arguments, 'url <url>' | 'file <file path>'")
        if args[0] == "url":
            engine.sigMapURL.emit(args[1])
            
        elif args[0] == "file":
            engine.sigMapFile.emit(args[1])
            
        else:
            raise InvalidCommandError(f"{args[0]} is not a valid map command! Use 'url' for an online map or 'file' for an offline map.")
        
    def get_auto_complete(self):
        return {"url": None, "file": None}

class DeviceCommand(Command):
    name = "device"

    async def run(self, engine, *args):
        if not args:
            raise InvalidCommandError(
                "Missing device command"
            )

        if args[0] == "scan":

            if len(args) < 2:
                raise InvalidCommandError(
                    "Missing scan type. Use 'ble' or 'usb'"
                )

            if args[1] == "usb":
                table = TableFormatter(
                    ["Serial", "Type"],
                    title="Detected USB Devices",
                )

                devices = RunManager.scan_all_usb()


                for device in devices:
                    table.add_row([
                        str(device.get("serial_number")),
                        str(device.get("product", "Unknown")),
                    ])

                return table.get_table()

            elif args[1] == "ble":
                table = TableFormatter(
                    ["Device"],
                    title="Detected BLE Devices",
                )

                engine.print_output(
                    "Starting bluetooth scan, "
                    f"duration "
                    f"{Settings.Advanced.headless_scan_length}s. "
                    "Please stand by for results."
                )

                devices = await RunManager.call(
                    RunManager._scan_bluetooth(
                        Settings.Advanced.headless_scan_length
                    )
                )

                if devices is None:
                    return (
                        "BLE scan failed, see log "
                        "(Check if bluetooth is on)"
                    )

                for device in devices:
                    table.add_row([str(device)])

                if devices:
                    engine.print_output("\n\n")
                    engine.print_output(
                        table.get_table()
                    )

                    if engine.headless:
                        engine.print_output(
                            "Press Enter to return..."
                        )

                return ""

            else:
                raise InvalidCommandError(
                    f"{args[1]} is not a valid argument! "
                    "Valid arguments are 'ble' | 'usb'"
                )

        elif args[0] == "connect":

            if len(args) < 2:
                raise InvalidCommandError(
                    "Missing connection type. "
                    "Use 'ble' or 'usb'"
                )

            if args[1] == "usb":

                connected_usb = RunManager.scan_all_usb()
                

                connections_found = []

                for conn_device in connected_usb:
                    product = conn_device.get("product")

                    if not product:
                        continue

                    for target_device in args[2:]:
                        if target_device.lower() in product.lower():

                            RunManager.add_device(
                                conn_device.get("serial_number"),
                                "radiacode",
                                ConnectionType.USB,
                            )

                            connections_found.append(
                                product
                            )

                            break

                if connections_found:
                    return (
                        "USB devices connected:\n"
                        + "\n".join(connections_found)
                    )

                return (
                    f"No USB devices matching {args[2:]}"
                )

            elif args[1] == "ble":

                engine.print_output(
                    f"Attempting connection to "
                    f"{len(args[2:])} devices..."
                )

                await RunManager.call(
                    RunManager.connect_bluetooth_list(
                        args[2:]
                    )
                )

                return ""

            else:
                raise InvalidCommandError(
                    f"{args[1]} is not a valid argument! "
                    "Valid arguments are 'ble' | 'usb'"
                )

    def get_auto_complete(self):
        return {
            "connect": {"usb": None, "ble": None},
            "disconnect": {"all": None}
            | {key: None for key in RunManager.device_registry.keys()},
            "scan": {"usb": None, "ble": None},
        }


class WatchCommand(Command):
    name = "watch"
    is_watching = False
    realtime_values_buffer: dict[str, WrappedRealTimePackage] = {}
    device_states_buffer: dict[str, WrappedStatusPackage] = {}

    def __init__(self):
        RunManager.Signals.currentUpdated.connect(self.update_realtime_values_buffer)
        RunManager.Signals.statusUpdated.connect(self.update_device_state_buffer)

    async def run(self, engine: ScriptEngine, *args):
        if not engine.headless:
            raise ActiveGUIError("watch")

        engine.sigCancelCurrent.connect(self.stop_watching)
        self.is_watching = True
        try:
            print_table("\033[2J\033[H")

            while self.is_watching:
                table = TableFormatter(
                    [
                        "Device",
                        "Count Rate [/s]",
                        "Dose Rate [uSv/s]",
                        "Battery [%]",
                        "Temp [deg C]",
                    ]
                )

                for device, realtime_values in self.realtime_values_buffer.items():
                    device_state = self.device_states_buffer.get(device)
                    if not device_state:
                        continue

                    table.add_row(
                        [
                            device,
                            round(realtime_values.CPS, 2),
                            round(realtime_values.DR, 3),
                            device_state.battery,
                            device_state.temperature,
                        ]
                    )

                print_table(table.get_table())

                await asyncio.sleep(Settings.Advanced.update_loop_delay)

        except asyncio.CancelledError:
            raise

    def update_realtime_values_buffer(
        self, device_name: str, data: WrappedRealTimePackage
    ):
        self.realtime_values_buffer[device_name] = data

    def update_device_state_buffer(self, device_name: str, data: WrappedStatusPackage):
        self.device_states_buffer[device_name] = data

    def stop_watching(self):
        self.is_watching = False


class ExecCommand(Command):
    name = "exec"

    async def run(self, engine: ScriptEngine, *args):
        if len(args) != 1:
            raise ArgumentError(
                "'exec' requires exactly one argument corresponding to a filepath"
            )

        pth = Path.home() / Path(args[0])

        if not pth.is_file:
            raise ArgumentError(f"{pth} does not exists")

        engine.suppress_output(True)
        table = TableFormatter(["Lines executed"], title=f"Executed {pth.name}")

        with open(str(pth)) as f:
            for line in f:
                table.add_row([line])
                await engine.queue.put(line)

        return table.get_table()


# ===== Register commands ======
def register_commands(registry: CommandRegistry):
    registry.register(ClearCommand())
    registry.register(DeviceCommand())
    registry.register(ExecCommand())
    registry.register(HelpCommand())
    registry.register(IndexCommand())
    registry.register(ListCommand())
    registry.register(MapCommand())
    registry.register(ViewCommand())
    registry.register(ROICommand())
    registry.register(SpectrogramCommand())
    registry.register(WatchCommand())
