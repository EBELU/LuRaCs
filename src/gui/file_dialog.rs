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

    #[cfg_attr(coverage_nightly, coverage(off))]
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

    #[cfg_attr(coverage_nightly, coverage(off))]
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

    #[cfg_attr(coverage_nightly, coverage(off))]
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

    #[cfg_attr(coverage_nightly, coverage(off))]
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
        for (cb, res) in app.file_dialogs.drain_completed() {
            cb(app, res);
        }
    }

    fn drain_completed(&mut self) -> Vec<(DialogCallback, DialogResult)> {
        if self.jobs.is_empty() {
            return Vec::new();
        }
        let jobs = std::mem::take(&mut self.jobs);
        let mut still_pending = Vec::new();
        let mut completed = Vec::new();

        for job in jobs {
            match job.rx.try_recv() {
                Ok(res) => completed.push((job.on_complete, res)),
                Err(mpsc::TryRecvError::Empty) => still_pending.push(job),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }

        self.jobs = still_pending;
        completed
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_dialog_queue_and_pending_file_slot() {
        let ctx = egui::Context::default();

        // PendingFileSlot: empty -> pending -> ready -> disconnected
        let mut slot: PendingFileSlot<i32> = PendingFileSlot::new();
        assert!(slot.take_ready().is_none());

        let (tx, rx) = mpsc::channel();
        slot.rx = Some(rx);
        assert!(slot.take_ready().is_none()); // Empty
        drop(tx);
        assert!(slot.take_ready().is_none()); // Disconnected

        slot.spawn(&ctx, || 123);
        let mut got = None;
        for _ in 0..50 {
            if let Some(v) = slot.take_ready() {
                got = Some(v);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(got, Some(123));

        // FileDialogQueue: empty, pending, disconnected, and completed
        let mut queue = FileDialogQueue::new();
        assert!(queue.drain_completed().is_empty());

        let (tx1, rx1) = mpsc::channel();
        queue.jobs.push(PendingJob {
            rx: rx1,
            on_complete: Box::new(|_, _| {}),
        });
        assert!(queue.drain_completed().is_empty()); // Empty stays pending
        assert_eq!(queue.jobs.len(), 1);
        drop(tx1);
        assert!(queue.drain_completed().is_empty()); // Disconnected is dropped
        assert!(queue.jobs.is_empty());

        queue.spawn(
            &ctx,
            || DialogResult::File(Some(PathBuf::from("test.n42"))),
            |_, _| {},
        );
        let mut completed = Vec::new();
        for _ in 0..50 {
            completed = queue.drain_completed();
            if !completed.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(completed.len(), 1);
        assert!(matches!(completed[0].1, DialogResult::File(Some(_))));
    }
}
