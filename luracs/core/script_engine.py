from PySide6.QtCore import QObject, Signal, Qt
import asyncio
import sys
import shlex
from collections import deque

from core import RunManager, Log, Settings, SpectrumManager
from utils.console_commands import register_commands, CommandRegistry

# --- Helpers ---
def clear_terminal():
    print("\033[2J\033[H", end="")




    

class ScriptEngine(QObject):
    sigCommandAppendOutput = Signal(str)
    sigCommandOutput = Signal(str)
    sigShutdown = Signal()

    def __init__(self, parent=None, headless=False, program_version=""):
        super().__init__(parent)
        self.queue = asyncio.Queue()
        self.headless = headless
        self.program_version = program_version
        self._loop = None
        self._tasks = []

        self.registry = CommandRegistry()
        register_commands(self.registry)

        if self.headless:
            self.sigCommandOutput.connect(self.print_output)

    # --- Startup ---
    async def start(self):
        self._loop = asyncio.get_running_loop()

        # Create background tasks
        self._tasks.append(asyncio.create_task(self._run()))

        if self.headless:
            self._tasks.append(asyncio.create_task(self._read_input()))

        self.queue.put_nowait("clear")  # Show welcome message

    # --- Optional stdin reader (debug/terminal mode) ---
    async def _read_input(self):
        try:
            while True:
                cmd = await self._loop.run_in_executor(None, sys.stdin.readline)
                if not cmd:
                    continue
                

                await self.queue.put(cmd.strip())
                if cmd.strip().lower() in ("exit", "quit", "shutdown"):
                    break

        except asyncio.CancelledError:
            return

    # --- Main command loop ---
    async def _run(self):
        try:
            while True:
                cmd = await self.queue.get()

                if cmd == "__exit__":
                    break

                cmd = cmd.strip()
                if not cmd:
                    continue
                
                await self.command_parser(cmd)

        except asyncio.CancelledError:
            return

    # --- Shutdown ---
    async def stop(self):
        await self.queue.put("__exit__")

        # cancel background tasks
        for task in self._tasks:
            task.cancel()

        # optionally wait for them to finish cleanly
        await asyncio.gather(*self._tasks, return_exceptions=True)

    # --- Sync-safe entry point (GUI / threads) ---
    def submit_from_sync(self, cmd: str):
        if self._loop is None:
            return

        self._loop.call_soon_threadsafe(
            self.queue.put_nowait,
            cmd
        )


    def print_output(self, text: str):
        clear_terminal()
        print(text)
        print("\nLuRaCs Console <<< ", end="", flush=True)

    # --- Command handling ---
    async def command_parser(self, cmd: str):
        commands = shlex.split(cmd)
        if not commands:
            return
        
        cmd_name = commands[0].lower()
        cmd_args = commands[1:]

        if cmd_name in ("exit", "quit", "shutdown"):
            self.sigCommandAppendOutput.emit("Shutting down...")
            self.sigShutdown.emit()
            return

        command = self.registry.get(cmd_name)
        if command:
            res = await command.run(self, *cmd_args)
            self.sigCommandOutput.emit(res if res is not None else "")
        else:
            self.sigCommandOutput.emit(f"Unknown command: {cmd_name}. Type 'help' for a list of commands.")



    