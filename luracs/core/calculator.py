from collections.abc import Callable

from PySide6.QtCore import QObject, QRunnable, QThreadPool, Signal


class WorkerSignals(QObject):
    finished = Signal(object)
    error = Signal(Exception)


class Worker(QRunnable):
    def __init__(self, fn, *args, **kwargs):
        super().__init__()
        self.fn = fn
        self.args = args
        self.kwargs = kwargs
        self.signals = WorkerSignals()

    def run(self):
        try:
            result = self.fn(*self.args, **self.kwargs)
            self.signals.finished.emit(result)
        except Exception as e:
            self.signals.error.emit(e)


class Calculator:
    """
    Calculator for running CPU-heavy calculations off the GUI thread.

    - Uses QRunnable + QThreadPool to execute functions in background threads.
    - Results and errors are delivered back to the GUI via Qt signals.
    - Ensures the GUI remains responsive even when running multiple optimizations.
    - Suitable for short, repeated tasks (e.g., function optimization with ~250 iterations).
    - No progress reporting; each task simply emits the final result when done.
    """

    _pool = QThreadPool.globalInstance()

    @classmethod
    def run(
        cls,
        fn: Callable,
        *args,
        on_result: Callable | None = None,
        on_error: Callable | None = None,
        **kwargs,
    ):
        """Run a function in a background thread and signal back the result."""
        worker = Worker(fn, *args, **kwargs)

        if on_result:
            worker.signals.finished.connect(on_result)
        if on_error:
            worker.signals.error.connect(on_error)

        cls._pool.start(worker)
