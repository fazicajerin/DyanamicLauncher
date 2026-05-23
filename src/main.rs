// Dynamic Launcher - main.rs
// Pure Rust, no HTML, no webview
// egui for UI, rdev for hotkey, rayon for fast indexing

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // no console in release

mod search;
mod watcher;

use eframe::egui::{
    self, Align, CentralPanel, Color32, FontId, Frame, Key, Layout,
    Modifiers, Pos2, Rect, RichText, Rounding, Stroke, TextEdit, Vec2,
    ViewportBuilder, ViewportCommand,
};
use search::{ResultKind, SearchEngine, SearchResult};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::process::Command;

// ── Colours ───────────────────────────────────────────────────────────────────
const BG:          Color32 = Color32::from_rgb(10,  10,  10);
const WIDGET_BG:   Color32 = Color32::from_rgb(17,  17,  17);
const BORDER_COL:  Color32 = Color32::from_rgb(42,  42,  42);
const TEXT_COL:    Color32 = Color32::from_rgb(240, 240, 240);
const DIM_COL:     Color32 = Color32::from_rgb(100, 100, 100);
const SELECT_COL:  Color32 = Color32::from_rgb(38,  38,  38);
const HOVER_COL:   Color32 = Color32::from_rgb(30,  30,  30);
const ACCENT_COL:  Color32 = Color32::from_rgb(255, 255, 255);

const WIDGET_W:    f32 = 580.0;
const COLLAPSED_H: f32 = 56.0;
const ITEM_H:      f32 = 54.0;
const MAX_ITEMS:   usize = 8;
const RADIUS:      f32 = 28.0;

// ── Shared hotkey flag ────────────────────────────────────────────────────────
static TOGGLE_FLAG: AtomicBool = AtomicBool::new(false);

fn main() {
    // ── Start hotkey listener thread (no admin needed with rdev) ─────────────
    std::thread::spawn(|| {
        use rdev::{listen, Event, EventType, Key};
        let mut ctrl_held = false;

        listen(move |event: Event| {
            match event.event_type {
                EventType::KeyPress(Key::ControlLeft)
                | EventType::KeyPress(Key::ControlRight) => {
                    ctrl_held = true;
                }
                EventType::KeyRelease(Key::ControlLeft)
                | EventType::KeyRelease(Key::ControlRight) => {
                    ctrl_held = false;
                }
                EventType::KeyPress(Key::Space) if ctrl_held => {
                    TOGGLE_FLAG.store(true, Ordering::Relaxed);
                }
                _ => {}
            }
        })
        .ok();
    });

    // ── Build search engine & start background indexer ────────────────────────
    let engine = Arc::new(SearchEngine::new().expect("Failed to init search engine"));

    // Index in background — doesn't block startup
    let engine_idx = Arc::clone(&engine);
    std::thread::spawn(move || {
        engine_idx.build_index();
    });

    // Start file watcher
    watcher::start_watcher(engine.db());

    // ── Launch egui window ────────────────────────────────────────────────────
    let screen_w = 1920.0_f32; // fallback; egui gives us real size at runtime
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size([WIDGET_W, COLLAPSED_H])
            .with_position(Pos2::new((screen_w - WIDGET_W) / 2.0, 18.0))
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_taskbar(false),
        ..Default::default()
    };

    eframe::run_native(
        "Seno",
        options,
        Box::new(|cc| Ok(Box::new(DynamicLauncherAPP::new(cc, engine)))),
    )
    .expect("Failed to start Seno");
}

// ══════════════════════════════════════════════════════════════════════════════
struct DynamicLauncherAPP {
    engine:        Arc<SearchEngine>,
    query:         String,
    results:       Vec<SearchResult>,
    selected:      usize,
    visible:       bool,
    target_h:      f32,
    current_h:     f32,
    last_query:    String,
}

impl DynamicLauncherAPP {
    fn new(_cc: &eframe::CreationContext, engine: Arc<SearchEngine>) -> Self {
        Self {
            engine,
            query:      String::new(),
            results:    vec![],
            selected:   0,
            visible:    false,
            target_h:   COLLAPSED_H,
            current_h:  COLLAPSED_H,
            last_query: String::new(),
        }
    }

    fn show(&mut self) {
        self.visible = true;
    }

    fn hide(&mut self) {
        self.visible    = false;
        self.query      = String::new();
        self.results    = vec![];
        self.selected   = 0;
        self.last_query = String::new();
        self.target_h   = COLLAPSED_H;
    }

    fn open_selected(&mut self) {
        if let Some(result) = self.results.get(self.selected) {
            let path = result.path.clone();
            self.engine.record_open(&path);
            std::thread::spawn(move || {
                #[cfg(target_os = "windows")]
                {
                    Command::new("cmd")
                        .args(["/C", "start", "", &path])
                        .spawn()
                        .ok();
                }
                #[cfg(target_os = "macos")]
                Command::new("open").arg(&path).spawn().ok();
                #[cfg(target_os = "linux")]
                Command::new("xdg-open").arg(&path).spawn().ok();
            });
            self.hide();
        }
    }

    fn update_search(&mut self) {
        if self.query == self.last_query {
            return;
        }
        self.last_query = self.query.clone();

        if self.query.trim().is_empty() {
            self.results  = vec![];
            self.selected = 0;
            self.target_h = COLLAPSED_H;
            return;
        }

        // Search is fast enough to do inline (tantivy-backed)
        self.results  = self.engine.search(&self.query);
        self.selected = 0;

        let n         = self.results.len().min(MAX_ITEMS);
        self.target_h = if n == 0 {
            COLLAPSED_H + 48.0          // "no results" row
        } else {
            COLLAPSED_H + 8.0 + n as f32 * ITEM_H + 10.0
        };
        self.target_h = self.target_h.min(COLLAPSED_H + MAX_ITEMS as f32 * ITEM_H + 20.0);
    }
}

// ── egui render loop ──────────────────────────────────────────────────────────
impl eframe::App for DynamicLauncherAPP {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0] // fully transparent background
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── poll hotkey flag ──────────────────────────────────────────────────
        if TOGGLE_FLAG.swap(false, Ordering::Relaxed) {
            if self.visible { self.hide(); } else { self.show(); }
        }

        // ── hide when not visible ─────────────────────────────────────────────
        if !self.visible {
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            return;
        }

        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Focus);

        // ── smooth height animation ───────────────────────────────────────────
        let diff = self.target_h - self.current_h;
        if diff.abs() > 0.5 {
            self.current_h += diff * 0.25;
            ctx.request_repaint();
        } else {
            self.current_h = self.target_h;
        }

        // Resize window to match animation
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::new(WIDGET_W, self.current_h)));

        // ── keyboard handling ─────────────────────────────────────────────────
        let input = ctx.input(|i| i.clone());

        if input.key_pressed(Key::Escape) {
            self.hide();
            return;
        }
        if input.key_pressed(Key::Enter) {
            self.open_selected();
            return;
        }
        if input.key_pressed(Key::ArrowDown) {
            if self.selected + 1 < self.results.len() {
                self.selected += 1;
            }
        }
        if input.key_pressed(Key::ArrowUp) {
            if self.selected > 0 {
                self.selected -= 1;
            }
        }

        // ── draw UI ───────────────────────────────────────────────────────────
        let style = ctx.style();
        CentralPanel::default()
            .frame(Frame::none())
            .show(ctx, |ui| {
                let full_rect = ui.available_rect_before_wrap();

                // ── outer pill background ─────────────────────────────────────
                ui.painter().rect(
                    full_rect,
                    Rounding::same(RADIUS),
                    WIDGET_BG,
                    Stroke::new(1.0, BORDER_COL),
                );

                // ── search row ────────────────────────────────────────────────
                ui.allocate_ui_with_layout(
                    Vec2::new(WIDGET_W, COLLAPSED_H),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.add_space(18.0);

                        // Search icon
                        ui.label(
                            RichText::new("⌕")
                                .color(DIM_COL)
                                .font(FontId::proportional(20.0)),
                        );

                        ui.add_space(10.0);

                        // Text input — auto-focused
                        let te = TextEdit::singleline(&mut self.query)
                            .frame(false)
                            .desired_width(WIDGET_W - 120.0)
                            .font(FontId::proportional(15.0))
                            .text_color(TEXT_COL)
                            .hint_text(
                                RichText::new("Search apps, files, folders…")
                                    .color(DIM_COL)
                                    .font(FontId::proportional(15.0)),
                            );

                        let response = ui.add(te);
                        response.request_focus();

                        // ESC hint
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(16.0);
                            ui.label(
                                RichText::new("esc")
                                    .color(DIM_COL)
                                    .font(FontId::proportional(11.0)),
                            );
                        });
                    },
                );

                // ── update search on every frame ──────────────────────────────
                self.update_search();

                // ── results ───────────────────────────────────────────────────
                if !self.results.is_empty() {
                    // divider
                    let div_y = full_rect.min.y + COLLAPSED_H - 1.0;
                    ui.painter().hline(
                        full_rect.min.x + 14.0..=full_rect.max.x - 14.0,
                        div_y,
                        Stroke::new(1.0, BORDER_COL),
                    );

                    ui.add_space(6.0);

                    for (i, result) in self.results.iter().enumerate().take(MAX_ITEMS) {
                        let is_selected = i == self.selected;
                        let row_bg = if is_selected { SELECT_COL } else { WIDGET_BG };

                        let (row_rect, row_resp) = ui.allocate_exact_size(
                            Vec2::new(WIDGET_W - 16.0, ITEM_H - 4.0),
                            egui::Sense::click(),
                        );

                        // hover bg
                        let bg = if row_resp.hovered() {
                            HOVER_COL
                        } else {
                            row_bg
                        };

                        ui.painter().rect_filled(
                            row_rect,
                            Rounding::same(8.0),
                            bg,
                        );

                        // icon
                        ui.painter().text(
                            Pos2::new(row_rect.min.x + 14.0, row_rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            result.kind.icon(),
                            FontId::proportional(16.0),
                            TEXT_COL,
                        );

                        // name
                        ui.painter().text(
                            Pos2::new(row_rect.min.x + 42.0, row_rect.center().y - 8.0),
                            egui::Align2::LEFT_CENTER,
                            &result.name,
                            FontId::proportional(13.0),
                            TEXT_COL,
                        );

                        // path (truncated)
                        let short_path = if result.path.len() > 58 {
                            format!("…{}", &result.path[result.path.len() - 55..])
                        } else {
                            result.path.clone()
                        };
                        ui.painter().text(
                            Pos2::new(row_rect.min.x + 42.0, row_rect.center().y + 10.0),
                            egui::Align2::LEFT_CENTER,
                            short_path,
                            FontId::proportional(10.0),
                            DIM_COL,
                        );

                        if row_resp.clicked() {
                            self.selected = i;
                            self.open_selected();
                            return;
                        }
                        if row_resp.hovered() {
                            self.selected = i;
                        }
                    }
                } else if !self.query.is_empty() {
                    // "no results" row
                    let div_y = full_rect.min.y + COLLAPSED_H - 1.0;
                    ui.painter().hline(
                        full_rect.min.x + 14.0..=full_rect.max.x - 14.0,
                        div_y,
                        Stroke::new(1.0, BORDER_COL),
                    );
                    ui.add_space(14.0);
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new("No results found")
                                .color(DIM_COL)
                                .font(FontId::proportional(13.0)),
                        );
                    });
                }
            });

        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}
