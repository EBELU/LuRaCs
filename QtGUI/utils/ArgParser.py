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
        type=str,
        help="Attempt bluetooth connections based on device names"
    )

    args = parser.parse_args()
    
    if args.bluetooth:
        loop = asyncio.get_event_loop()
        loop.create_task(RunManager.connect_bluetooth_list(args.bluetooth.split(" ")))
        
            