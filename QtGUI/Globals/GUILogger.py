import logging

gui_logger = logging.getLogger("Program")

# Create the logger
gui_logger = logging.getLogger("Program")
gui_logger.setLevel(logging.DEBUG)  # or INFO, WARNING, etc.

# Create a console handler (or QtHandler if you use a widget)
console_handler = logging.StreamHandler()
console_handler.setLevel(logging.DEBUG)

# Formatter: HH:MM:SS LEVEL MESSAGE
formatter = logging.Formatter(fmt="%(asctime)s %(levelname)s %(message)s",
                              datefmt="%H:%M:%S")

console_handler.setFormatter(formatter)