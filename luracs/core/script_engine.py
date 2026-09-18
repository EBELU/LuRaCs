import asyncio
import shlex
import threading
import traceback
from concurrent.futures import Future

from prompt_toolkit import PromptSession
from prompt_toolkit.completion import NestedCompleter
from PySide6.QtCore import QObject, Signal

from .script_engine_components.commands import register_commands
from .script_engine_components.exceptions import (
    ActiveGUIError,
    ArgumentError,
    InvalidCommandError,
)
from .script_engine_components.registry import CommandRegistry


def clear_terminal():
    print("\033[2J\033[H", end="")       

class ThreadBridge(QObject):
    "Communication between the script engine thread and gui thread for functions where it is required"
    sig_restart_spectrogram = Signal(str)
    sig_start_spectrogram = Signal(str, str, int, int)
    sig_close_spectrogram = Signal(str)
    
    def __init__(self, parent):
        super().__init__(parent=parent)
        
        from luracs.spectrogram import restart_spectrogram, start_spectrogram

        from .run_manager import RunManager
        
        self.sig_start_spectrogram.connect(start_spectrogram)
        self.sig_restart_spectrogram.connect(restart_spectrogram)
        self.sig_close_spectrogram.connect(RunManager.close_spectrogram)
        
    def restart_spectrogram(self, db_name: str):
        self.sig_restart_spectrogram.emit(db_name)
        
    def start_spectrogram(self, db_name, device: str, save_interval: int, concat: int):
        self.sig_start_spectrogram.emit(
            db_name,
            device,
            save_interval,
            concat
        )
    
    def close_spectrogram(self, db_name: str):
        self.sig_close_spectrogram.emit(db_name)

class ScriptEngine(QObject):
    sigCommandAppendOutput = Signal(str)
    sigCommandOutput = Signal(str)
    sigShutdown = Signal()
    sigCancelCurrent = Signal()
    sigClearConsole = Signal(str)

    sigMapURL = Signal(str)
    sigMapFile = Signal(str)

    def __init__(
        self,
        parent=None,
        headless: bool = False,
        program_version: str = "",
        IS_H3: bool = False,
    ):
        super().__init__(parent)
        self.thread_bridge = ThreadBridge(None)

        self.headless = headless
        self.IS_H3 = IS_H3
        self.program_version = program_version

        self._thread = threading.Thread(
            target=self._thread_main,
            name="ScriptEngine",
            daemon=True,
        )

        self._loop: asyncio.AbstractEventLoop | None = None
        self._loop_ready = threading.Event()

        # These are created in the ScriptEngine thread
        self.queue: asyncio.Queue | None = None
        self._tasks = []
        self._current_command_task: asyncio.Task | None = None

        self.output_suppressed = False
        self.get_log_buffer = None
        self.console_cleared = True

        self.registry = CommandRegistry()
        register_commands(self.registry)

        self.auto_completer = None
        self.session = None

        if self.headless:
            self.auto_completer = self.make_autocompleter()
            self.session = PromptSession(
                completer=self.auto_completer
            )

        self._thread.start()
        self._loop_ready.wait()

    # ------------------------------------------------------------------
    # Thread / asyncio infrastructure
    # ------------------------------------------------------------------

    def _thread_main(self):
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)

        self._loop = loop
        self.queue = asyncio.Queue()

        self._loop_ready.set()

        try:
            loop.run_forever()
        finally:
            loop.run_until_complete(loop.shutdown_asyncgens())
            loop.close()

    def submit_to_thread(self, coro) -> Future:
        """Submit an asyncio coroutine to the ScriptEngine thread."""

        if self._loop is None:
            raise RuntimeError("ScriptEngine thread is not running")

        return asyncio.run_coroutine_threadsafe(
            coro,
            self._loop,
        )

    def call_soon_threadsafe(self, callback, *args):
        if self._loop is None:
            raise RuntimeError("ScriptEngine thread is not running")

        self._loop.call_soon_threadsafe(callback, *args)

    # ------------------------------------------------------------------
    # Startup
    # ------------------------------------------------------------------

    def start(self):
        return self.submit_to_thread(self._start())

    async def _start(self):
        self._tasks.append(
            asyncio.create_task(self._run())
        )

        if self.headless:
            self._tasks.append(
                asyncio.create_task(self._read_input())
            )

        self.queue.put_nowait(
            f"clear {self.headless}"
        )

    # ------------------------------------------------------------------
    # Input
    # ------------------------------------------------------------------

    async def _read_input(self):
        try:
            while True:
                try:
                    self.session.completer = (
                        self.make_autocompleter()
                    )

                    await asyncio.sleep(0.1)

                    cmd = await self.session.prompt_async(
                        "LuRaCs Console <<< "
                    )

                except KeyboardInterrupt:
                    self.sigCancelCurrent.emit()
                    self.cancel_current_command()
                    self.queue.put_nowait("clear")
                    continue

                if (
                    self._current_command_task
                    and not self._current_command_task.done()
                ):
                    self.sigCancelCurrent.emit()
                    self.cancel_current_command()
                    self.queue.put_nowait("clear")

                if not cmd:
                    continue

                await self.queue.put(cmd.strip())

                if cmd.strip().lower() in (
                    "exit",
                    "quit",
                    "shutdown",
                ):
                    break

        except asyncio.CancelledError:
            self.cancel_current_command()

    # ------------------------------------------------------------------
    # Command loop
    # ------------------------------------------------------------------

    async def _run(self):
        try:
            while True:
                cmd = await self.queue.get()

                try:
                    if cmd == "__exit__":
                        break

                    cmd = cmd.strip()

                    if not cmd:
                        continue

                    await self.command_parser(cmd)

                finally:
                    self.queue.task_done()

        except asyncio.CancelledError:
            pass

    # ------------------------------------------------------------------
    # Commands from GUI / other threads
    # ------------------------------------------------------------------

    def submit_from_sync(self, cmd: str):
        """
        Thread-safe way to submit a command.
        """
        if self._loop is None:
            return

        self._loop.call_soon_threadsafe(
            self.queue.put_nowait,
            cmd,
        )

    # ------------------------------------------------------------------
    # Command handling
    # ------------------------------------------------------------------

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
            self.sigCommandAppendOutput.emit(
                "Shutting down..."
            )
            self.sigShutdown.emit()
            return

        if cmd_name == "clear":
            self.cancel_current_command()
            self.sigClearConsole.emit("")

        command = self.registry.get(cmd_name)

        if not command:
            self.sigCommandOutput.emit(
                f"Unknown command: {cmd_name}. "
                f"Type 'help' for a list of commands."
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

        except (
            InvalidCommandError,
            ArgumentError,
            ActiveGUIError,
        ) as e:
            res = f"{type(e).__name__}: {e}"

        except Exception:
            res = traceback.format_exc()

        finally:
            self._current_command_task = None

        self.print_output(res)

        if cmd_name == "clear":
            self.console_cleared = True

    # ------------------------------------------------------------------
    # Shutdown
    # ------------------------------------------------------------------

    def stop(self):
        """
        Thread-safe synchronous request to stop the engine.
        """
        return self.submit_to_thread(
            self._stop()
        )

    async def _stop(self):
        if self.queue:
            await self.queue.put("__exit__")

        for task in self._tasks:
            task.cancel()

        await asyncio.gather(
            *self._tasks,
            return_exceptions=True,
        )

        self._tasks.clear()

        loop = asyncio.get_running_loop()
        loop.stop()

    # ------------------------------------------------------------------
    # Misc
    # ------------------------------------------------------------------

    def make_autocompleter(self) -> dict:
        command_args = {"exit": None}

        for cmd in self.registry.commands.values():
            command_args[cmd.name] = (
                cmd.get_auto_complete()
            )

        return NestedCompleter.from_nested_dict(
            command_args
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

            self.sigCommandOutput.emit(
                text if text else ""
            )

    def cancel_current_command(self):
        if (
            self._current_command_task
            and not self._current_command_task.done()
        ):
            self._current_command_task.cancel()

    def connect_log_buffer(self, get_log_fn):
        self.get_log_buffer = get_log_fn

    def suppress_output(self, state: bool):
        self.output_suppressed = state
