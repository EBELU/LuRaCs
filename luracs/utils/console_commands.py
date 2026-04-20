from abc import ABC, abstractmethod

from core import RunManager, SpectrumManager

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
    

# ====== Commands ======
class ClearCommand(Command):
    name = "clear"
    async def run(self, engine, *args):
        default_message = (
f""" ======  ======  ======     
|71    ||88    ||55    |    Version:  {engine.program_version}
|  Lu  ||  Ra  ||  Cs  |    LuRaCs Console
| 177  || 226  || 137  |    Type 'help' for a list of commands
======  ======  ======     
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
            "view: \n"
            "   - view spectrum <spectrum_name | spectrum number>: Display a loaded spectrum\n"
            "   - view device <device_name | device number>: Display information about a connected device\n"
            "   - view logs [lines = 50]: Display recent log entries\n"
            "watch: \n"
            "   - watch spectrum <spectrum_name | spectrum number>: Continuously display updates for a spectrum\n"
            "   - watch device <device_name | device number>: Continuously display updates for a device\n"
            "   - watch logs: Continuously display new log entries\n"
        )
        return help_text

class ListCommand(Command):
    name = "list"
    async def run(self, engine, *args):
        if not args:
            return "Usage: list [devices|spectra]"

        if args[0] == "devices":
            spectra = [f"{i} - {key}" for i, key in enumerate(RunManager.devices.keys())]
            return "\n".join(["Loaded devices:"] + spectra) if spectra else "No devices connected."
        
        elif args[0] == "spectra":
            spectra = [f"{i} - {key}" for i, key in enumerate(SpectrumManager.get_spectra_dict().keys())]
            return "\n".join(["Loaded spectra:"] + spectra) if spectra else "No spectrum loaded."
        
        else:
            return "Unknown list option. Use 'list devices' or 'list spectra'."

# ===== Register commands ======
def register_commands(registry: CommandRegistry):
    registry.register(ClearCommand())
    registry.register(HelpCommand())
    registry.register(ListCommand())