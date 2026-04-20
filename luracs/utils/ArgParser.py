from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from main import MainWindow
import argparse
import os
import asyncio
from pathlib import Path
from core import Log, RunManager, SpectrumManager
from utils.file_io import xml_parser
from utils.file_io import io_dispatcher


def parse_cli_args(main_window: MainWindow):
    parser = argparse.ArgumentParser(
        description="""LuRaCs -- Lund Radiation analysis Computer software"""
    )

    parser.add_argument(
        "-db", "--debug", action="store_true", help="Launch in debug mode"
    )
    parser.add_argument(
        "-is", "--import_spectrum", nargs="+", help="Load spectrum files"
    )

    parser.add_argument(
        "--nuclides", nargs="+", help="Preset nuclides shown"
    )

    parser.add_argument("--headless", action="store_true", help="Run without GUI (terminal mode)")

    parser.add_argument("-l", "--load", nargs="+", help="Load spectrum files")

    parser.add_argument("--import_rois", type=str, help="Load ROI from an xml file")

    parser.add_argument("-roi", nargs=2, type=float, action="append")

    parser.add_argument(
        "-bt",
        "--bluetooth",
        nargs="+",
        help="Attempt bluetooth connections based on device names",
    )

    parser.add_argument(
        "-usb", nargs="+", help="Attempt USB connections based on device names"
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
                    loop.create_task(
                        RunManager.add_device(
                            conn_device.get("serial_number"), "radiacode", True
                        )
                    )

    if args.import_spectrum:
        for pth in args.import_spectrum:
            path = Path(pth)
            if path.is_file():
                if str(path).endswith(".xml") or str(path).endswith(".n42"):
                    parser = io_dispatcher(path)
                    SpectrumManager.import_spectrum(parser.data)

    if args.roi:
        for roi_bounds in args.roi:
            SpectrumManager.ROIManager.add_roi(roi_bounds[0], roi_bounds[1])
    
    if args.nuclides:
        for nuclide in args.nuclides:
            main_window.isotopics_tab.set_nuclide_check(nuclide, True)
