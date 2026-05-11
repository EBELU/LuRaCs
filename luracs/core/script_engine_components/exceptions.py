class InvalidCommandError(Exception):
    def __init__(self, command, message="Invalid command"):
        self.command = command
        super().__init__(f"Command: {command} - {message}")
        
class ArgumentError(Exception):
    def __init__(self, command):
        super().__init__(str(command))
        
class ActiveGUIError(Exception):
    def __init__(self, command):
        super().__init__(f"{command} is only available when running in headless mode")