from PySide6.QtCore import QObject, Signal, Qt
import asyncio
import shlex
import traceback

from .script_engine_components.exceptions import ArgumentError, InvalidCommandError, ActiveGUIError
from .script_engine_components.registry import CommandRegistry
from .script_engine_components.commands import register_commands

from prompt_toolkit import PromptSession
from prompt_toolkit.completion import NestedCompleter
from .settings import Settings


# --- Helpers ---
def clear_terminal():
    print("\033[2J\033[H", end="")


class ScriptEngine(QObject):
    """
    The script engine converts text commands to actions for the application using async. It is the backbone of headless mode.
    """
    sigCommandAppendOutput = Signal(str)
    sigCommandOutput = Signal(str)
    sigShutdown = Signal()
    sigCancelCurrent = Signal()
    sigClearConsole = Signal(str)

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
        self.auto_completer = None
        self.console_cleared = True
        if self.headless:               
            self.auto_completer = self.make_autocompleter()
            self.session = PromptSession(completer=self.auto_completer)

    # --- Startup ---
    async def start(self):
        self._loop = asyncio.get_running_loop()

        # Create background tasks
        self._tasks.append(asyncio.create_task(self._run()))

        if self.headless:
            self._tasks.append(asyncio.create_task(self._read_input()))

        self.queue.put_nowait(f"clear {self.headless}")  # Show welcome message
    
    
    def make_autocompleter(self) -> dict:
        command_args = {"exit": None}
        for cmd in self.registry.commands.values():
            command_args[cmd.name] = cmd.get_auto_complete()
        return NestedCompleter.from_nested_dict(command_args)
        
        
    async def _read_input(self):
        try:
            while True:
                try:
                    # Update the autocompleter
                    self.session.completer = self.make_autocompleter()
                    
                    # await self.queue.join()
                    
                    await asyncio.sleep(0.1) # Wait so the input is under the displayed output
                    cmd = await self.session.prompt_async("LuRaCs Console <<< ")


                except KeyboardInterrupt:
                    # Cancel whatever is going on but dont lock up the program
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
            

        except asyncio.CancelledError:
            # Stop!
            self.cancel_current_command()
            return

    # --- Main command loop ---
    async def _run(self):
        "Run the execution loop"
        try:
            while True:
                cmd = await self.queue.get()

                if cmd == "__exit__":
                    break

                cmd = cmd.strip()
                if not cmd:
                    continue
                
                try:
                    await self.command_parser(cmd)
                finally:
                    self.queue.task_done()

        except asyncio.CancelledError:
            return
        


    # --- Shutdown ---
    async def stop(self):
        await self.queue.put("__exit__")

        # cancel background tasks
        for task in self._tasks:
            task.cancel()

        # wait for them to finish cleanly
        await asyncio.gather(*self._tasks, return_exceptions=True)

    # --- Sync-safe entry point ---
    def submit_from_sync(self, cmd: str):
        "Put a command in the execution queue"
        if self._loop is None:
            return

        self._loop.call_soon_threadsafe(
            self.queue.put_nowait,
            cmd
        )

    def print_output(self, text: str):
        if self.output_suppressed:
            return
        
        if self.headless:
            if text:
                clear_terminal()
            
            print(text)
        else:
            if self.console_cleared:
                self.sigClearConsole.emit("")
                self.console_cleared = False

            self.sigCommandOutput.emit(text if text else "")


    # --- Command handling ---
    async def command_parser(self, cmd: str):
        "Where the dough is made"
        if self._current_command_task:
            self.sigCancelCurrent.emit()
            self.cancel_current_command()
        
        commands = shlex.split(cmd) # Split it like a unix shell
        if not commands:
            return

        cmd_name = commands[0].lower()
        cmd_args = commands[1:]

        # Shutdown?
        if cmd_name in ("exit", "quit", "shutdown"):
            self.sigCommandAppendOutput.emit("Shutting down...")
            self.sigShutdown.emit()
            return

        if cmd_name == "clear":
            self.cancel_current_command()
            self.sigClearConsole.emit("")
            

        command = self.registry.get(cmd_name)
        if not command:
            # If the command does not exist give some help
            self.sigCommandOutput.emit(
                f"Unknown command: {cmd_name}. Type 'help' for a list of commands."
            )
            return

        res = None
        try:
            # Run the command and catch the result
            self._current_command_task = asyncio.create_task(
                command.run(self, *cmd_args)
            )

            res = await self._current_command_task

        except asyncio.CancelledError:
            pass

        except (InvalidCommandError, ArgumentError, ActiveGUIError) as e:
            # These are errors defined to help with the execution of the command
            # They do not constitute a real error or crash
            res = f"{type(e).__name__}: {e}"

        except Exception:
            # If something crashes for real give a proper traceback
            res = traceback.format_exc()

        finally:
            self._current_command_task = None

        
        self.print_output(res)
            
        if cmd_name == "clear":
            self.console_cleared = True
            
    def cancel_current_command(self):
        if self._current_command_task and not self._current_command_task.done():
            self._current_command_task.cancel()
    
    
    def connect_log_buffer(self, get_log_fn):
        self.get_log_buffer = get_log_fn
        
        
    def suppress_output(self, state: bool):
        self.output_suppressed = state
        



    