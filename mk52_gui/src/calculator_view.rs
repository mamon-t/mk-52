//! Главное окно — «нативный» калькулятор МК-52: 10-разрядный ВФД и
//! физическая клавиатура 8 × 4, скомпонованная по фотографии оригинала.

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
const WIN_W: f32 = 1200.0;
const WIN_H: f32 = 541.0;
const BODY_X: f32 = 24.0;
const BODY_Y: f32 = 60.0;
const BODY_W: f32 = 1152.0;
const BODY_H: f32 = 418.0;
const DIGIT_W: f32 = 37.0;
const DIGIT_H: f32 = 132.0;
const KEY_W: f32 = 52.0;
const KEY_H: f32 = 40.0;
const KEY_PITCH_X: f32 = 69.0;
const KEY_PITCH_Y: f32 = 70.0;
const KEYPAD_COLS: usize = 8;
const KEYPAD_ROWS: usize = 4;
const KEYPAD_X: f32 = 561.5;
const KEYPAD_Y: f32 = 108.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyStyle {
    Gold,
    Cyan,
    Dark,
    Silver,
    Red,
}

impl KeyStyle {
    fn background(self) -> u32 {
        match self {
            Self::Gold => 0xe2d669,
            Self::Cyan => 0x38b5d8,
            Self::Dark => 0x18181a,
            Self::Silver => 0xa8a6a4,
            Self::Red => 0xc15043,
        }
    }

    fn foreground(self) -> u32 {
        match self {
            Self::Gold | Self::Cyan | Self::Silver => 0x171715,
            Self::Dark | Self::Red => 0xf2efe8,
        }
    }

    fn border(self) -> u32 {
        match self {
            Self::Gold => 0xf0e58d,
            Self::Cyan => 0x75d8ef,
            Self::Dark => 0x3b3b3d,
            Self::Silver => 0x7d7b78,
            Self::Red => 0xe17a6c,
        }
    }
}

#[derive(Clone, Copy)]
enum LegendColor {
    Gold,
    Cyan,
    Red,
}

impl LegendColor {
    fn color(self) -> u32 {
        match self {
            Self::Gold => 0xe7cf67,
            Self::Cyan => 0x63cfe8,
            Self::Red => 0xe07a6d,
        }
    }
}

struct KeyDef {
    /// Основная надпись на клавише.
    primary: &'static str,
    /// Дополнительная надпись над клавишей.
    secondary: &'static str,
    legend: LegendColor,
    style: KeyStyle,
    /// Логическая позиция в матрице ядра 10 × 5.
    logical: (usize, usize),
}

const fn key(
    primary: &'static str,
    secondary: &'static str,
    legend: LegendColor,
    style: KeyStyle,
    logical: (usize, usize),
) -> KeyDef {
    KeyDef {
        primary,
        secondary,
        legend,
        style,
        logical,
    }
}

/// Физическая раскладка МК-52: 8 колонок × 4 ряда.
///
/// Порядок клавиш повторяет оригинальную клавиатуру. Логические координаты
/// остаются в матрице ядра 10 × 5, поэтому ядро не зависит от внешнего вида.
const KEYPAD: [[KeyDef; KEYPAD_COLS]; KEYPAD_ROWS] = [
    [
        key("F", "", LegendColor::Gold, KeyStyle::Gold, (6, 3)),
        key("ШГ→", "x<0", LegendColor::Gold, KeyStyle::Dark, (7, 0)),
        key("П→Х", "10ˣ", LegendColor::Cyan, KeyStyle::Dark, (7, 1)),
        key("7", "sin", LegendColor::Gold, KeyStyle::Silver, (1, 1)),
        key("8", "cos", LegendColor::Gold, KeyStyle::Silver, (1, 2)),
        key("9", "tg", LegendColor::Gold, KeyStyle::Silver, (1, 3)),
        key("−", "x²", LegendColor::Cyan, KeyStyle::Silver, (2, 1)),
        key("÷", "1/x", LegendColor::Cyan, KeyStyle::Silver, (2, 3)),
    ],
    [
        key("K", "", LegendColor::Cyan, KeyStyle::Cyan, (6, 4)),
        key("ШГ←", "x=0", LegendColor::Gold, KeyStyle::Dark, (7, 2)),
        key("Х←П", "eˣ", LegendColor::Cyan, KeyStyle::Dark, (7, 3)),
        key("4", "sin⁻¹", LegendColor::Gold, KeyStyle::Silver, (0, 3)),
        key("5", "cos⁻¹", LegendColor::Gold, KeyStyle::Silver, (0, 4)),
        key("6", "tg⁻¹", LegendColor::Gold, KeyStyle::Silver, (1, 0)),
        key("+", "xʸ", LegendColor::Cyan, KeyStyle::Silver, (2, 0)),
        key("×", "10ˣ", LegendColor::Cyan, KeyStyle::Silver, (2, 2)),
    ],
    [
        key("↓↑", "x>0", LegendColor::Gold, KeyStyle::Dark, (7, 4)),
        key("В/О", "x≤0", LegendColor::Gold, KeyStyle::Dark, (3, 1)),
        key("БП", "x≥0", LegendColor::Gold, KeyStyle::Dark, (3, 0)),
        key("1", "√", LegendColor::Gold, KeyStyle::Silver, (0, 0)),
        key("2", "x²", LegendColor::Cyan, KeyStyle::Silver, (0, 1)),
        key("3", "x³", LegendColor::Cyan, KeyStyle::Silver, (0, 2)),
        key("↔", "π", LegendColor::Cyan, KeyStyle::Silver, (8, 4)),
        key("В↑", "x!", LegendColor::Cyan, KeyStyle::Silver, (9, 0)),
    ],
    [
        key("A↑", "", LegendColor::Gold, KeyStyle::Dark, (8, 0)),
        key("С/П", "", LegendColor::Gold, KeyStyle::Dark, (3, 2)),
        key("ПП", "", LegendColor::Gold, KeyStyle::Dark, (8, 1)),
        key("0", "xʸ", LegendColor::Cyan, KeyStyle::Silver, (1, 4)),
        key(".", "lg", LegendColor::Gold, KeyStyle::Silver, (8, 2)),
        key("/−/", "АВТ", LegendColor::Red, KeyStyle::Silver, (2, 4)),
        key("ВП", "ПРГ", LegendColor::Red, KeyStyle::Silver, (8, 3)),
        key("СХ", "СЕ", LegendColor::Red, KeyStyle::Red, (5, 2)),
    ],
];

pub struct CalculatorView {
    backend: Entity<EmuBackend>,
    /// Текущие символы разрядов ВФД (снимки приходят из потока эмуляции).
    digits: Vec<char>,
    powered: bool,
    /// Последний полный снимок для панели разработчика.
    snapshot: Option<DebugSnapshot>,
    show_panel: bool,
    paused: bool,
    turbo: bool,
    status: Option<String>,
    /// Положение переключателя углов: 0 — Р, 1 — ГРД, 2 — Г.
    angle_mode: usize,
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
            angle_mode: 1,
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

    fn set_angle_mode(&mut self, mode: usize, cx: &mut Context<Self>) {
        self.angle_mode = mode;
        cx.notify();
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
            .w_full()
            .h_full()
            .flex()
            .flex_row()
            .gap_1()
            .p_2()
            .bg(rgb(0x06110f))
            .rounded(px(8.0))
            .border_1()
            .border_color(rgb(0x45504c))
            .children(self.digits.iter().map(|ch| {
                let shown = if self.powered { *ch } else { ' ' };
                div()
                    .w(px(DIGIT_W))
                    .h(px(DIGIT_H))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(rgb(0x0b211d))
                    .rounded(px(3.0))
                    .text_size(px(54.0))
                    .text_color(rgb(0x63f5dc)) // бирюзовое свечение ВФД
                    .child(shown.to_string())
            }))
    }

    fn render_key(&self, row: usize, col: usize, key: &KeyDef) -> AnyElement {
        let (logical_row, logical_col) = key.logical;
        let primary_len = key.primary.chars().count();
        let text_size = if matches!(key.style, KeyStyle::Gold | KeyStyle::Cyan) {
            18.0
        } else if primary_len > 2 {
            11.0
        } else if matches!(key.style, KeyStyle::Dark) {
            12.0
        } else {
            16.0
        };
        div()
            .w(px(KEY_PITCH_X))
            .h(px(KEY_PITCH_Y))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .h(px(26.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(8.0))
                    .text_color(rgb(key.legend.color()))
                    .child(key.secondary),
            )
            .child(
                div()
                    .id(("key", row * KEYPAD_COLS + col))
                    .w(px(KEY_W))
                    .h(px(KEY_H))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(rgb(key.style.background()))
                    .rounded(px(5.0))
                    .border_1()
                    .border_color(rgb(key.style.border()))
                    .shadow_md()
                    .cursor_pointer()
                    .text_size(px(text_size))
                    .text_color(rgb(key.style.foreground()))
                    .on_mouse_down(MouseButton::Left, {
                        let be = self.backend.clone();
                        move |_ev, _win, cx| {
                            be.update(cx, |b, _| b.key_press(logical_row, logical_col));
                        }
                    })
                    .on_mouse_up(MouseButton::Left, {
                        let be = self.backend.clone();
                        move |_ev, _win, cx| {
                            be.update(cx, |b, _| b.key_release(logical_row, logical_col));
                        }
                    })
                    .child(key.primary),
            )
            .into_any()
    }

    fn render_angle_buttons(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        ["Р", "ГРД", "Г"]
            .into_iter()
            .enumerate()
            .map(|(mode, label)| {
                let active = self.angle_mode == mode;
                div()
                    .id(("angle", mode))
                    .px_2()
                    .py_1()
                    .rounded(px(3.0))
                    .bg(if active { rgb(0xbfa94f) } else { rgb(0x3a3b3d) })
                    .text_color(if active { rgb(0x1a1810) } else { rgb(0xd8d3c8) })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.set_angle_mode(mode, cx)))
                    .child(label)
                    .into_any()
            })
            .collect()
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
            )
            .child(
                button("mode-eggog", "F1 ЕГГОГ".into()).on_click(cx.listener(|this, _, _, cx| {
                    this.backend.update(cx, |b, _| b.key_press(6, 0));
                })),
            )
            .child(
                button("mode-darkness", "F2 Тьма".into()).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.backend.update(cx, |b, _| b.key_press(6, 1));
                    },
                )),
            )
            .child(
                button("mode-dummy", "F3 Думпель".into()).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.backend.update(cx, |b, _| b.key_press(6, 2));
                    },
                )),
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
            .h_full()
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
            .rounded(px(4.0))
            .bg(if self.powered {
                rgb(0x2e7d32)
            } else {
                rgb(0xa33a30)
            })
            .text_color(rgb(0xffffff))
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.toggle_power(cx)))
            .child(if self.powered { "ВКЛ" } else { "ВЫКЛ" });

        let angle_buttons = self.render_angle_buttons(cx);
        let controls = div()
            .absolute()
            .left(px(52.0))
            .top(px(286.0))
            .w(px(424.0))
            .h(px(92.0))
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgb(0x242526))
            .rounded(px(8.0))
            .border_1()
            .border_color(rgb(0x3b3c3e))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(rgb(0xbdb8ac))
                            .child("РЕЖИМ УГЛОВ"),
                    )
                    .child(div().flex().flex_row().gap_1().children(angle_buttons)),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(rgb(0xbdb8ac))
                            .child("ПИТАНИЕ"),
                    )
                    .child(power_btn),
            );

        let keypad = div()
            .absolute()
            .left(px(KEYPAD_X))
            .top(px(KEYPAD_Y))
            .w(px(KEY_PITCH_X * KEYPAD_COLS as f32))
            .h(px(KEY_PITCH_Y * KEYPAD_ROWS as f32))
            .flex()
            .flex_col()
            .children(KEYPAD.iter().enumerate().map(|(row, keys)| {
                div().flex().flex_row().h(px(KEY_PITCH_Y)).children(
                    keys.iter()
                        .enumerate()
                        .map(|(col, key)| self.render_key(row, col, key)),
                )
            }));

        let panel_toggle = div()
            .id("panel-toggle")
            .absolute()
            .left(px(1000.0))
            .top(px(492.0))
            .px_3()
            .py_1()
            .rounded(px(4.0))
            .bg(if self.show_panel {
                rgb(0x6b5a2e)
            } else {
                rgb(0xd8d2c6)
            })
            .text_color(if self.show_panel {
                rgb(0xfff4cf)
            } else {
                rgb(0x5a564d)
            })
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.toggle_panel(cx)))
            .child(if self.show_panel {
                "ПАНЕЛЬ ВКЛ"
            } else {
                "ПАНЕЛЬ"
            });

        let mut body = div()
            .absolute()
            .left(px(BODY_X))
            .top(px(BODY_Y))
            .w(px(BODY_W))
            .h(px(BODY_H))
            .relative()
            .bg(rgb(0x2b2c2e))
            .rounded(px(14.0))
            .border_1()
            .border_color(rgb(0x4a4b4d))
            .shadow_md()
            .child(
                div()
                    .absolute()
                    .left(px(210.0))
                    .top(px(44.0))
                    .text_size(px(21.0))
                    .text_color(rgb(0xd8d2c4))
                    .child("ЭЛЕКТРОНИКА  МК-52"),
            )
            .child(
                div()
                    .absolute()
                    .left(px(52.0))
                    .top(px(108.0))
                    .w(px(424.0))
                    .h(px(154.0))
                    .child(self.render_display()),
            )
            .child(controls)
            .child(keypad);

        if self.show_panel {
            body = body.child(
                div()
                    .absolute()
                    .left(px(36.0))
                    .top(px(74.0))
                    .w(px(520.0))
                    .h(px(330.0))
                    .child(self.render_panel(cx)),
            );
        }

        div()
            .relative()
            .size_full()
            .bg(rgb(0xefeae0))
            .child(body)
            .child(panel_toggle)
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
            title: Some("Электроника МК-52".into()),
            ..Default::default()
        }),
        is_resizable: false,
        window_min_size: Some(size(px(WIN_W), px(WIN_H))),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_keypad_maps_to_unique_logical_cells() {
        let mut seen = std::collections::HashSet::new();
        for row in &KEYPAD {
            for key in row {
                assert!(
                    key.logical.0 < mk52_core::KB_ROWS,
                    "{}: строка вне матрицы ядра",
                    key.primary
                );
                assert!(
                    key.logical.1 < mk52_core::KB_COLS,
                    "{}: столбец вне матрицы ядра",
                    key.primary
                );
                assert!(
                    seen.insert(key.logical),
                    "{}: логическая позиция {:?} уже занята",
                    key.primary,
                    key.logical
                );
            }
        }
        assert_eq!(seen.len(), KEYPAD_COLS * KEYPAD_ROWS);
    }

    #[test]
    fn service_keys_keep_core_addresses() {
        assert_eq!(KEYPAD[2][1].primary, "В/О");
        assert_eq!(KEYPAD[2][1].logical, (3, 1));
        assert_eq!(KEYPAD[3][1].primary, "С/П");
        assert_eq!(KEYPAD[3][1].logical, (3, 2));
    }
}
