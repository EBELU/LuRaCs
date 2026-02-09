import argparse

def parse_cli_args():
    parser = argparse.ArgumentParser(
        description="Command line interface for the ... can be used to quickly start the application in a certain state."

    )

    parser.add_argument(
        "-db", "--debug",
        type=bool,
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

