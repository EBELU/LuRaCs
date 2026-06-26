class CommandRegistry:
    def __init__(self):
        self.commands = {}

    def register(self, command):
        self.commands[command.name] = command

    def get(self, name):
        return self.commands.get(name)
