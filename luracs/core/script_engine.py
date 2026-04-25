from PySide6.QtCore import QObject, Signal, Qt
import asyncio
import sys
import shlex
import readline
from collections import deque
import traceback

from core import RunManager, Log, Settings, SpectrumManager
from utils.console_commands import register_commands, CommandRegistry, ArgumentError, InvalidCommandError, ActiveGUIError

from prompt_toolkit import PromptSession, print_formatted_text
from prompt_toolkit.patch_stdout import patch_stdout


# --- Helpers ---
def clear_terminal():
    print("\033[2J\033[H", end="")


class ScriptEngine(QObject):
    sigCommandAppendOutput = Signal(str)
    sigCommandOutput = Signal(str)
    sigShutdown = Signal()
    sigCancelCurrent = Signal()

    def __init__(self, parent=None, headless=False, program_version=""):
        super().__init__(parent)
        self.queue = asyncio.Queue()
        self.headless = headless
        self.program_version = program_version
        self._loop = None
        self._tasks = []
        self._current_command_task = None
        self.output_suppressed = False
        
        self.get_log_buffer = None

        self.registry = CommandRegistry()
        register_commands(self.registry)

        if self.headless:
            self.sigCommandOutput.connect(self.print_output)
            self.session = PromptSession()

    # --- Startup ---
    async def start(self):
        self._loop = asyncio.get_running_loop()

        # Create background tasks
        self._tasks.append(asyncio.create_task(self._run()))

        if self.headless:
            self._tasks.append(asyncio.create_task(self._read_input()))

        self.queue.put_nowait(f"clear {self.headless}")  # Show welcome message

    async def _read_input(self):
        try:
            while True:
                try:
                    cmd = await self.session.prompt_async("LuRaCs Console <<< ")
                except KeyboardInterrupt:
                    self.sigCancelCurrent.emit()
                    self.cancel_current_command()
                    self.queue.put_nowait("clear")
                    continue
                
                if self._current_command_task and not self._current_command_task.done():
                    self.sigCancelCurrent.emit()
                    self.cancel_current_command()
                    self.queue.put_nowait("clear")


                if not cmd:
                    continue
                await self.queue.put(cmd.strip())

                if cmd.strip().lower() in ("exit", "quit", "shutdown"):
                    break
                
                await asyncio.sleep(0.1)

        except asyncio.CancelledError:
            self.cancel_current_command()
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
        if self.output_suppressed:
            return
        
        if text:
            clear_terminal()
        print(text)


    # --- Command handling ---
    async def command_parser(self, cmd: str):
        if self._current_command_task:
            self.sigCancelCurrent.emit()
            self.cancel_current_command()
        commands = shlex.split(cmd)
        if not commands:
            return

        cmd_name = commands[0].lower()
        cmd_args = commands[1:]

        if cmd_name in ("exit", "quit", "shutdown"):
            self.sigCommandAppendOutput.emit("Shutting down...")
            self.sigShutdown.emit()
            return

        if cmd_name == "clear":
            self.cancel_current_command()

        command = self.registry.get(cmd_name)
        if not command:
            self.sigCommandOutput.emit(
                f"Unknown command: {cmd_name}. Type 'help' for a list of commands."
            )
            return

        res = None
        try:
            self._current_command_task = asyncio.create_task(
                command.run(self, *cmd_args)
            )

            res = await self._current_command_task

        except asyncio.CancelledError:
            pass

        except (InvalidCommandError, ArgumentError, ActiveGUIError) as e:
            res = f"{type(e).__name__}: {e}"

        except Exception:
            res = traceback.format_exc()

        finally:
            self._current_command_task = None

        
        self.sigCommandOutput.emit(res if res else "")
            
    def cancel_current_command(self):
        if self._current_command_task and not self._current_command_task.done():
            self._current_command_task.cancel()
    
    def connect_log_buffer(self, get_log_fn):
        self.get_log_buffer = get_log_fn
        
    def suppress_output(self, state: bool):
        self.output_suppressed = state
        



    