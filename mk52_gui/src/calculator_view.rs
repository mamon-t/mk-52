//! Главное окно — «нативный» калькулятор МК-52: 10-разрядный ВFD + матрица клавиш.

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Bounds, Entity, MouseButton, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, px, rgb, size,
};
use std::time::Duration;

use mk52_core::DebugSnapshot;

use crate::backend::BackendMessage;
use crate::{EmuBackend, display_cells};

/// Размер окна и геометрия элементов (в px логического пространства gpui).
const WIN_W: f32 = 460.0;
const WIN_H: f32 = 860.0;
const DIGIT_W: f32 = 34.0;
const DIGIT_H: f32 = 64.0;
const KEY_W: f32 = 66.0;
const KEY_H: f32 = 48.0;

pub struct CalculatorView {
    backend: Entity<EmuBackend>,
    /// Текущие символы разрядов ВFD (снимки приходят из потока эмуляции).
    digits: Vec<char>,
    powered: bool,
    /// Последний полный снимок для панели разработчика.
    snapshot: Option<DebugSnapshot>,
    show_panel: bool,
    paused: bool,
    turbo: bool,
    status: Option<String>,
}

impl CalculatorView {
    pub fn new(backend: &Entity<EmuBackend>, cx: &mut Context<Self>) -> Self {
        let backend = backend.clone();
        let view = Self {
            backend: backend.clone(),
            digits: vec![' '; 10],
            powered: false,
            snapshot: None,
            show_panel: false,
            paused: false,
            turbo: false,
            status: None,
        };

        // Снимки публикуются фоновым потоком; UI опрашивает слот с частотой кадра.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;

                let update = this.update(cx, |this, cx| {
                    if let Some(snap) = this.backend.read(cx).poll_snapshot() {
                        this.digits = display_cells(&snap);
                        this.powered = snap.powered;
                        this.snapshot = Some(snap);
                        cx.notify();
                    }
                    if let Some(msg) = this.backend.read(cx).poll_message() {
                        this.status = Some(match msg {
                            BackendMessage::Saved(text) => {
                                format!("Сохранено: {} строк программы", text.lines().count())
                            }
                            BackendMessage::Loaded(n) => format!("Загружено слов: {n}"),
                            BackendMessage::LoadError(e) => format!("Ошибка: {e}"),
                        });
                        cx.notify();
                    }
                });
                if update.is_err() {
                    break;
                }
            }
        })
        .detach();

        view
    }

    fn toggle_panel(&mut self, cx: &mut Context<Self>) {
        self.show_panel = !self.show_panel;
        cx.notify();
    }

    fn toggle_power(&self, cx: &mut Context<Self>) {
        self.backend.update(cx, |b, _| b.toggle_power());
    }

    fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        self.paused = !self.paused;
        let paused = self.paused;
        self.backend.update(cx, |b, _| b.set_paused(paused));
        cx.notify();
    }

    fn toggle_turbo(&mut self, cx: &mut Context<Self>) {
        self.turbo = !self.turbo;
        let turbo = self.turbo;
        self.backend.update(cx, |b, _| b.set_turbo(turbo));
        cx.notify();
    }

    fn step_once(&mut self, cx: &mut Context<Self>) {
        self.paused = true;
        self.backend.update(cx, |b, _| {
            b.set_paused(true);
            b.step_once();
        });
        cx.notify();
    }

    fn load_demo(&mut self, cx: &mut Context<Self>) {
        self.backend.update(cx, |b, _| {
            b.load_program("000 /АВ\n001 /В/П\n002 /КХ".into());
        });
    }

    fn save_program(&mut self, cx: &mut Context<Self>) {
        self.backend.update(cx, |b, _| b.request_save_program());
    }

    fn render_display(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .gap_1()
            .p_2()
            .bg(rgb(0x0a0a0a))
            .rounded(px(8.0))
            .border_1()
            .border_color(rgb(0x333333))
            .children(self.digits.iter().map(|ch| {
                let shown = if self.powered { *ch } else { ' ' };
                div()
                    .w(px(DIGIT_W))
                    .h(px(DIGIT_H))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(rgb(0x101510))
                    .rounded(px(4.0))
                    .text_size(px(40.0))
                    .text_color(rgb(0xff9a3c)) // янтарное свечение ВFD
                    .child(shown.to_string())
            }))
    }

    fn render_key(&self, row: usize, col: usize, label: &'static str) -> AnyElement {
        let bg = if label.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            rgb(0xd8d0c0) // цифровые — светлые, как у оригинала
        } else {
            rgb(0x8f9aa5)
        };
        div()
            .id(("key", row * mk52_core::KB_COLS + col))
            .w(px(KEY_W))
            .h(px(KEY_H))
            .flex()
            .items_center()
            .justify_center()
            .bg(bg)
            .rounded(px(6.0))
            .shadow_md()
            .cursor_pointer()
            .text_size(px(15.0))
            .text_color(rgb(0x1a1a1a))
            .on_mouse_down(MouseButton::Left, {
                let be = self.backend.clone();
                move |_ev, _win, cx| {
                    be.update(cx, |b, _| b.key_press(row, col));
                }
            })
            .on_mouse_up(MouseButton::Left, {
                let be = self.backend.clone();
                move |_ev, _win, cx| {
                    be.update(cx, |b, _| b.key_release(row, col));
                }
            })
            .child(label.into_element().into_any())
            .into_any()
    }

    fn render_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(snap) = self.snapshot.as_ref() else {
            return div()
                .p_2()
                .text_size(px(12.0))
                .text_color(rgb(0xb0b0b0))
                .child("Снимок ещё не получен")
                .into_any();
        };

        let button = |id: &'static str, label: String| {
            div()
                .id(id)
                .px_2()
                .py_1()
                .rounded(px(4.0))
                .bg(rgb(0x4a505a))
                .text_color(rgb(0xffffff))
                .cursor_pointer()
                .child(label)
        };
        let controls = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(
                button(
                    "pause",
                    if self.paused {
                        "Продолжить"
                    } else {
                        "Пауза"
                    }
                    .into(),
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_pause(cx))),
            )
            .child(
                button(
                    "turbo",
                    format!("Турбо: {}", if self.turbo { "вкл" } else { "выкл" }),
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_turbo(cx))),
            )
            .child(
                button("step", "Шаг".into())
                    .on_click(cx.listener(|this, _, _, cx| this.step_once(cx))),
            )
            .child(
                button("demo", "Демо-программа".into())
                    .on_click(cx.listener(|this, _, _, cx| this.load_demo(cx))),
            )
            .child(
                button("save", "Выгрузить".into())
                    .on_click(cx.listener(|this, _, _, cx| this.save_program(cx))),
            );

        let pc = crate::base_converter::convert(u64::from(snap.pc));
        let text = format!(
            "{}\n{}\n{}\n{}\nPC {:03}: {} = ${} = 0b{} | IR ${:02X}",
            crate::snapshot::summary(snap),
            crate::snapshot::registers_table(snap),
            crate::snapshot::memory_dump(snap),
            crate::snapshot::vfd_table(snap),
            snap.pc,
            pc.decimal,
            pc.hex,
            pc.binary,
            snap.instruction,
        );
        let mut panel = div()
            .id("dev-panel")
            .w_full()
            .max_h(px(390.0))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded(px(6.0))
            .bg(rgb(0x171a1f))
            .border_1()
            .border_color(rgb(0x3a404a))
            .text_size(px(11.0))
            .text_color(rgb(0xd6d6d6))
            .child(controls)
            .child(text);
        if let Some(status) = &self.status {
            panel = panel.child(div().text_color(rgb(0x9fd39f)).child(status.clone()));
        }
        panel.into_any()
    }
}

impl Render for CalculatorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let power_btn = div()
            .id("power")
            .px_3()
            .py_1()
            .rounded(px(6.0))
            .bg(if self.powered {
                rgb(0x2e7d32)
            } else {
                rgb(0xb3261e)
            })
            .text_color(rgb(0xffffff))
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.toggle_power(cx)))
            .child(if self.powered { "Вкл" } else { "Выкл" });

        let panel_btn = div()
            .id("panel-toggle")
            .px_3()
            .py_1()
            .rounded(px(6.0))
            .bg(if self.show_panel {
                rgb(0x2f6f9f)
            } else {
                rgb(0x4a505a)
            })
            .text_color(rgb(0xffffff))
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.toggle_panel(cx)))
            .child(if self.show_panel {
                "Панель вкл"
            } else {
                "Панель"
            });

        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .gap_3()
            .p_4()
            .bg(rgb(0x2b2f36))
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(18.0))
                            .text_color(rgb(0xe6e6e6))
                            .child("Электроника МК-52"),
                    )
                    .child(div().flex().gap_2().child(panel_btn).child(power_btn)),
            )
            .child(self.render_display())
            .child(div().flex().flex_col().gap_1().children(
                crate::KEY_LABELS.iter().enumerate().map(|(r, row_labels)| {
                    div().flex().flex_row().gap_1().children(
                        row_labels
                            .iter()
                            .enumerate()
                            .map(|(c, lbl)| self.render_key(r, c, lbl)),
                    )
                }),
            ));
        if self.show_panel {
            root = root.child(self.render_panel(cx));
        }
        root
    }
}

/// Параметры окна калькулятора.
pub fn window_options(cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(WIN_W), px(WIN_H)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("МК-52 — Эмулятор".into()),
            ..Default::default()
        }),
        window_min_size: Some(size(px(380.0), px(700.0))),
        ..Default::default()
    }
}
