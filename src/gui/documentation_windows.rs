//! Documentation and Bibliography windows.
//!
//! Ported from `luracs/gui/windows/documentation_windows.py`.

use std::path::{Path, PathBuf};

use eframe::egui;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
struct DocEntry {
    label: String,
    path: PathBuf,
}

pub struct DocumentationWindow {
    pub open: bool,
    entries: Vec<DocEntry>,
    selected_idx: usize,
    cached_content: String,
    loaded_dir: Option<PathBuf>,
}

impl Default for DocumentationWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentationWindow {
    pub const fn new() -> Self {
        Self {
            open: false,
            entries: Vec::new(),
            selected_idx: 0,
            cached_content: String::new(),
            loaded_dir: None,
        }
    }

    pub const fn open(&mut self) {
        self.open = true;
    }

    fn ensure_loaded(&mut self, doc_dir: &Path) {
        if self.loaded_dir.as_deref() == Some(doc_dir) {
            return;
        }
        self.loaded_dir = Some(doc_dir.to_path_buf());
        self.entries = collect_doc_entries(doc_dir);
        self.selected_idx = self
            .entries
            .iter()
            .position(|e| e.label == "Welcome")
            .unwrap_or(0);
        self.load_selected();
    }

    fn load_selected(&mut self) {
        self.cached_content = self
            .entries
            .get(self.selected_idx)
            .and_then(|e| std::fs::read_to_string(&e.path).ok())
            .unwrap_or_else(|| "Documentation file not found.".to_string());
    }

    pub fn render(&mut self, ui: &egui::Ui, doc_dir: &Path) {
        if !self.open {
            return;
        }
        self.ensure_loaded(doc_dir);

        let mut close_requested = false;
        let viewport_id = egui::ViewportId::from_hash_of("documentation_window");
        let builder = egui::ViewportBuilder::default()
            .with_title("Documentation")
            .with_inner_size([800.0, 600.0])
            .with_resizable(false);

        ui.ctx()
            .show_viewport_immediate(viewport_id, builder, |ui, _class| {
                if ui.input(|i| i.viewport().close_requested()) {
                    close_requested = true;
                }

                egui::Panel::bottom("doc_win_buttons")
                    .resizable(false)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Close").clicked() {
                                        close_requested = true;
                                    }
                                },
                            );
                        });
                    });

                egui::Panel::left("doc_win_tree")
                    .resizable(true)
                    .default_size(220.0)
                    .min_size(140.0)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("doc_tree_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let mut clicked_idx = None;
                                for (idx, entry) in self.entries.iter().enumerate() {
                                    if ui
                                        .selectable_label(idx == self.selected_idx, &entry.label)
                                        .clicked()
                                    {
                                        clicked_idx = Some(idx);
                                    }
                                }
                                if let Some(idx) = clicked_idx {
                                    self.selected_idx = idx;
                                    self.load_selected();
                                }
                            });
                    });

                egui::CentralPanel::default().show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("doc_content_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            render_markdown(ui, &self.cached_content);
                        });
                });
            });

        if close_requested {
            self.open = false;
        }
    }
}

pub struct BibliographyWindow {
    pub open: bool,
    cached_content: String,
    loaded_path: Option<PathBuf>,
}

impl Default for BibliographyWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl BibliographyWindow {
    pub const fn new() -> Self {
        Self {
            open: false,
            cached_content: String::new(),
            loaded_path: None,
        }
    }

    pub const fn open(&mut self) {
        self.open = true;
    }

    fn ensure_loaded(&mut self, md_path: &Path) {
        if self.loaded_path.as_deref() == Some(md_path) {
            return;
        }
        self.loaded_path = Some(md_path.to_path_buf());
        self.cached_content = std::fs::read_to_string(md_path)
            .unwrap_or_else(|e| format!("Could not load {}: {e}", md_path.display()));
    }

    pub fn render(&mut self, ui: &egui::Ui, md_path: &Path) {
        if !self.open {
            return;
        }
        self.ensure_loaded(md_path);

        let mut close_requested = false;
        let viewport_id = egui::ViewportId::from_hash_of("bibliography_window");
        let builder = egui::ViewportBuilder::default()
            .with_title("Bibliography")
            .with_inner_size([800.0, 600.0])
            .with_resizable(false);

        ui.ctx()
            .show_viewport_immediate(viewport_id, builder, |ui, _class| {
                if ui.input(|i| i.viewport().close_requested()) {
                    close_requested = true;
                }

                egui::Panel::bottom("bib_win_buttons")
                    .resizable(false)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Close").clicked() {
                                        close_requested = true;
                                    }
                                },
                            );
                        });
                    });

                egui::CentralPanel::default().show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("bib_content_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            render_markdown(ui, &self.cached_content);
                        });
                });
            });

        if close_requested {
            self.open = false;
        }
    }
}

fn collect_doc_entries(doc_dir: &Path) -> Vec<DocEntry> {
    let mut entries = Vec::new();
    for entry in WalkDir::new(doc_dir).into_iter().flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !ext.eq_ignore_ascii_case("md") {
            continue;
        }
        let rel = path.strip_prefix(doc_dir).unwrap_or(path);
        let label = rel
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/");
        entries.push(DocEntry {
            label,
            path: path.to_path_buf(),
        });
    }
    entries.sort_by(|a, b| a.label.cmp(&b.label));
    entries
}

/// Lightweight markdown renderer for headings, bullets, code blocks, and links.
fn render_markdown(ui: &mut egui::Ui, md: &str) {
    let mut in_code = false;
    let mut code_buf = String::new();

    for line in md.lines() {
        let trimmed = line.trim_end();
        if trimmed.starts_with("```") {
            if in_code {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.monospace(code_buf.trim_end());
                });
                code_buf.clear();
                in_code = false;
            } else {
                in_code = true;
            }
            continue;
        }
        if in_code {
            code_buf.push_str(line);
            code_buf.push('\n');
            continue;
        }

        if trimmed.is_empty() {
            ui.add_space(4.0);
        } else if let Some(rest) = trimmed.strip_prefix("# ") {
            ui.add_space(6.0);
            ui.heading(rest.trim());
            ui.separator();
        } else if let Some(rest) = trimmed.strip_prefix("## ") {
            ui.add_space(6.0);
            ui.strong(egui::RichText::new(rest.trim()).size(16.0));
        } else if let Some(rest) = trimmed.strip_prefix("### ") {
            ui.add_space(4.0);
            ui.strong(rest.trim());
        } else if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* "))
        {
            ui.horizontal_wrapped(|ui| {
                ui.label("•");
                render_inline_links(ui, rest.trim());
            });
        } else {
            render_inline_links(ui, trimmed);
        }
    }

    if in_code && !code_buf.is_empty() {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.monospace(code_buf.trim_end());
        });
    }
}

fn render_inline_links(ui: &mut egui::Ui, text: &str) {
    if let (Some(lb), Some(rb)) = (text.find('['), text.find("]("))
        && rb > lb
        && let Some(rp_rel) = text[rb + 2..].find(')')
    {
        let label = &text[lb + 1..rb];
        let url = &text[rb + 2..rb + 2 + rp_rel];
        let before = &text[..lb];
        let after = &text[rb + 2 + rp_rel + 1..];
        ui.horizontal_wrapped(|ui| {
            if !before.is_empty() {
                ui.label(before);
            }
            ui.hyperlink_to(label, url);
            if !after.is_empty() {
                ui.label(after);
            }
        });
        return;
    }
    ui.label(text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_doc_entries_and_bibliography() {
        let docs = collect_doc_entries(Path::new("resources/docs/documentation"));
        assert!(docs.iter().any(|e| e.label == "Welcome"));
        assert!(Path::new("resources/docs/bibliography.md").is_file());
    }
}
