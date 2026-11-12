//! Приложение эмулятора «Электроника МК-52» на GPUI Kit.
//!
//! Главное окно — «нативный» калькулятор: 10-разрядный ВФД и матрица клавиш.
//! Бэкенд эмуляции живёт в отдельном потоке и публикует снимки состояния;
//! представление опрашивает их с частотой кадров.

mod backend;
mod base_converter;
mod calculator_view;
mod snapshot;

use gpui_kit::prelude::*;
use mk52_core::{DebugSnapshot, darkness_blanks_digit};

use crate::backend::EmuBackend;

/// Раскладка клавиатуры МК-52 для отображения: строка x столбец -> подпись клавиши.
pub const KEY_LABELS: [[&str; mk52_core::KB_COLS]; mk52_core::KB_ROWS] = [
    ["1", "2", "3", "4", "5"],
    ["6", "7", "8", "9", "0"],
    ["+", "-", "×", "÷", "±"],
    ["AB", "В/П", "С/П", "КХ", "PX"],
    ["DX", "EX", "FX", "GX", "HX"],
    ["IX", "JX", "CX", "VX", "ZX"],
    ["F1", "F2", "F3", "F4", "F5"],
    ["F6", "F7", "F8", "F9", "F10"],
    ["F11", "F12", "F13", "F14", "F15"],
    ["F16", "F17", "F18", "F19", "F20"],
];

/// Снимок для отрисовки главного окна: символы разрядов с учётом спецрежимов.
pub fn display_cells(snap: &DebugSnapshot) -> Vec<char> {
    snap.vfd_codes
        .iter()
        .enumerate()
        .map(|(d, &code)| {
            if darkness_blanks_digit(
                match snap.special_mode.as_deref() {
                    Some("Тьма") => Some(mk52_core::SpecialMode::Darkness),
                    _ => None,
                },
                d,
            ) {
                ' '
            } else {
                mk52_core::VfdDisplay::decode(code)
            }
        })
        .collect()
}

fn main() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);

        let backend = cx.new(|_| EmuBackend::spawn());
        gpui_kit::open_window(calculator_view::window_options(cx), cx, |_, cx| {
            cx.new(|cx| calculator_view::CalculatorView::new(&backend, cx))
        })
        .expect("не удалось открыть главное окно МК-52");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use mk52_core::{CoreEvent, Machine};

    #[test]
    fn display_cells_respects_darkness() {
        let mut m = Machine::new();
        m.handle_event(CoreEvent::PowerOn);
        m.vpu.special_mode = Some(mk52_core::SpecialMode::Darkness);
        m.display.service_digit(0, 0x2);
        m.display.service_digit(1, 0x2);
        let cells = display_cells(&m.debug_snapshot());
        assert_eq!(cells[0], '1');
        assert_eq!(cells[1], ' ');
    }
}
