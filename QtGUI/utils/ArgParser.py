import argparse
import asyncio
from ..Globals import Log, RunManager

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
        "-l", "--load",
        type=list,
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
    
        
            