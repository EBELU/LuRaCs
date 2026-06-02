import logging

# Create the logger
gui_logger = logging.getLogger("Application")
gui_logger.setLevel(logging.DEBUG)

# Create a console handler
console_handler = logging.StreamHandler()
console_handler.setLevel(logging.DEBUG)

# Formatter: HH:MM:SS LEVEL MESSAGE
formatter = logging.Formatter(
    fmt="%(asctime)s %(levelname)s %(message)s", datefmt="%H:%M:%S"
)

console_handler.setFormatter(formatter)
