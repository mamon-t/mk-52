//! Приложение эмулятора «Электроника МК-52» на GPUI Kit.
//!
//! Главное окно — «нативный» калькулятор: 10-разрядный ВФД и физическая
//! клавиатура МК-52 (8 × 4), отображаемая по фотографии оригинала.
//! Бэкенд эмуляции живёт в отдельном потоке и публикует снимки состояния;
//! представление опрашивает их с частотой кадров.

mod backend;
mod base_converter;
mod calculator_view;
mod snapshot;

use gpui_kit::prelude::*;
use mk52_core::{DebugSnapshot, darkness_blanks_digit};

use crate::backend::EmuBackend;

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
