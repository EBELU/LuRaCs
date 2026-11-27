//! Non-blocking file dialog runner.
//!
//! Running `rfd::FileDialog` synchronously on the main UI thread blocks the
//! `eframe`/`winit` event loop, which causes Wayland/X11 compositors to flag the
//! application as "not responding" while the user browses files. These helpers
//! run `rfd::FileDialog` on a background thread and deliver the result back on
//! the main thread via `std::sync::mpsc`, waking `egui` with `request_repaint()`.

use std::path::PathBuf;
use std::sync::mpsc;

use eframe::egui;

use crate::app::LuRaCsApp;

enum DialogResult {
    File(Option<PathBuf>),
    Files(Option<Vec<PathBuf>>),
}

type DialogCallback = Box<dyn FnOnce(&mut LuRaCsApp, DialogResult)>;

struct PendingJob {
    rx: mpsc::Receiver<DialogResult>,
    on_complete: DialogCallback,
}

/// Queue of in-flight file dialogs owned by `LuRaCsApp`.
#[derive(Default)]
pub struct FileDialogQueue {
    jobs: Vec<PendingJob>,
}

impl FileDialogQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pick_file(
        &mut self,
        ctx: &egui::Context,
        dialog: rfd::FileDialog,
        on_complete: impl FnOnce(&mut LuRaCsApp, Option<PathBuf>) + 'static,
    ) {
        self.spawn(
            ctx,
            move || DialogResult::File(dialog.pick_file()),
            |app, res| {
                if let DialogResult::File(path) = res {
                    on_complete(app, path);
                }
            },
        );
    }

    pub fn pick_files(
        &mut self,
        ctx: &egui::Context,
        dialog: rfd::FileDialog,
        on_complete: impl FnOnce(&mut LuRaCsApp, Option<Vec<PathBuf>>) + 'static,
    ) {
        self.spawn(
            ctx,
            move || DialogResult::Files(dialog.pick_files()),
            |app, res| {
                if let DialogResult::Files(paths) = res {
                    on_complete(app, paths);
                }
            },
        );
    }

    pub fn save_file(
        &mut self,
        ctx: &egui::Context,
        dialog: rfd::FileDialog,
        on_complete: impl FnOnce(&mut LuRaCsApp, Option<PathBuf>) + 'static,
    ) {
        self.spawn(
            ctx,
            move || DialogResult::File(dialog.save_file()),
            |app, res| {
                if let DialogResult::File(path) = res {
                    on_complete(app, path);
                }
            },
        );
    }

    pub fn pick_folder(
        &mut self,
        ctx: &egui::Context,
        dialog: rfd::FileDialog,
        on_complete: impl FnOnce(&mut LuRaCsApp, Option<PathBuf>) + 'static,
    ) {
        self.spawn(
            ctx,
            move || DialogResult::File(dialog.pick_folder()),
            |app, res| {
                if let DialogResult::File(path) = res {
                    on_complete(app, path);
                }
            },
        );
    }

    fn spawn(
        &mut self,
        ctx: &egui::Context,
        run_dialog: impl FnOnce() -> DialogResult + Send + 'static,
        on_complete: impl FnOnce(&mut LuRaCsApp, DialogResult) + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let res = run_dialog();
            drop(tx.send(res));
            ctx.request_repaint();
        });
        self.jobs.push(PendingJob {
            rx,
            on_complete: Box::new(on_complete),
        });
    }

    /// Poll all in-flight dialogs and run callbacks for any that finished.
    pub fn poll(app: &mut LuRaCsApp) {
        if app.file_dialogs.jobs.is_empty() {
            return;
        }
        let jobs = std::mem::take(&mut app.file_dialogs.jobs);
        let mut still_pending = Vec::new();
        let mut completed = Vec::new();

        for job in jobs {
            match job.rx.try_recv() {
                Ok(res) => completed.push((job.on_complete, res)),
                Err(mpsc::TryRecvError::Empty) => still_pending.push(job),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }

        app.file_dialogs.jobs = still_pending;
        for (cb, res) in completed {
            cb(app, res);
        }
    }
}

/// Lightweight single-slot async dialog receiver for standalone windows that
/// don't take `&mut LuRaCsApp` in `render`.
pub struct PendingFileSlot<T> {
    rx: Option<mpsc::Receiver<T>>,
}

impl<T> Default for PendingFileSlot<T> {
    fn default() -> Self {
        Self { rx: None }
    }
}

impl<T> PendingFileSlot<T> {
    pub const fn new() -> Self {
        Self { rx: None }
    }
}

impl<T: Send + 'static> PendingFileSlot<T> {
    pub fn spawn(&mut self, ctx: &egui::Context, run: impl FnOnce() -> T + Send + 'static) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let res = run();
            drop(tx.send(res));
            ctx.request_repaint();
        });
        self.rx = Some(rx);
    }

    pub fn take_ready(&mut self) -> Option<T> {
        let rx = self.rx.as_ref()?;
        match rx.try_recv() {
            Ok(val) => {
                self.rx = None;
                Some(val)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.rx = None;
                None
            }
        }
    }
}
