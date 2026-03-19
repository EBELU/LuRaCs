import argparse
import os
import asyncio
from pathlib import Path
from core import Log, RunManager, SpectrumManager
from .file_io import xml_io
from GUI_components.import_export import io_dispatcher

def parse_cli_args():
    
    Log.info("Parsing CLI args")
    parser = argparse.ArgumentParser(
        description="Command line interface for the ... can be used to quickly start the application in a certain state."

    )

    parser.add_argument(
        "-db", "--debug",
        action="store_true",
        help="Launch in debug mode"
    )
    parser.add_argument(
        "-is", "--import_spectrum",
        nargs="+",
        help="Load spectrum files"
    )
    
    parser.add_argument(
        "-l", "--load",
        nargs="+",
        help="Load spectrum files"
    )

    parser.add_argument(
        "-roi",
        type=str,
        help="Load a roi json file"
    )
    
    parser.add_argument(
        "-bt", "--bluetooth",
        nargs="+",
        help="Attempt bluetooth connections based on device names"
    )
    
    parser.add_argument(
        "-usb",
        nargs="+",
        help="Attempt USB connections based on device names"
    )

    args = parser.parse_args()
    
    if args.debug:
        loop = asyncio.get_event_loop()
        loop.create_task(RunManager.add_device("None", "mock"))
    
    if args.bluetooth:
        loop = asyncio.get_event_loop()
        Log.info(f"Initializing BLE devices: {args.bluetooth}")
        loop.create_task(RunManager.connect_bluetooth_list(args.bluetooth))
        
    if args.usb:
        connected_usb = RunManager.scan_all_usb()
        loop = asyncio.get_event_loop()
        Log.info(f"Initializing USB devices: {args.usb}")
        
        for conn_device in connected_usb:
            for target_device in args.usb:
                if target_device.lower() in conn_device.get("product").lower():
                    loop.create_task(RunManager.add_device(conn_device.get("serial_number"), "radiacode", True))
    
    if args.import_spectrum:
        for pth in args.import_spectrum:
            path = Path(pth)
            if path.is_file():
                if str(path).endswith(".xml") or str(path).endswith(".n42"):
                    parser = io_dispatcher(path)
                    SpectrumManager.create_spectrum(parser.kwargs["name"], parser.kwargs["foreground"].channels)
                    SpectrumManager.set_foreground_spectrum(parser.kwargs["name"], parser.kwargs["foreground"])
                    
                    if "background" in parser.kwargs:
                        SpectrumManager.set_background_spectrum(parser.kwargs["name"], parser.kwargs["background"])
                        
                    if "calibration" in parser.kwargs:
                        SpectrumManager.calibrate_spectrum(parser.kwargs["name"], parser.kwargs["calibration"])
    
        
            