//! Вспомогательный процессор (ВПУ): FSM обслуживания клавиатуры и индикатора,
//! а также обработка недокументированных функций МК-52.
//!
//! Согласно документации:
//!  - КС в начале цикла сканирует строки клавиатуры; нажатая клавиша кодируется
//!    псевдокомандой и записывается ГП в кольцевые регистры как адрес перехода;
//!  - в конце цикла ВПУ загружает коды разрядов в ВFD (мультиплексирование);
//!  - «EGGOG», «Тьма» и «пустышка» — побочные эффекты переполнения счётчика
//!    программ / особого ввода, воспроизводимые цикло-точно.

use crate::clock::ClockController;
use crate::peripheral::{Keyboard, VfdDisplay};
use crate::regfile::RegisterFile;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuxState {
    Idle,
    /// Сканирование строки `row` клавиатуры (такты 0..99).
    ScanKeyboard {
        row: usize,
    },
    /// Формирование кода нажатой клавиши и запрос шины для записи адреса перехода.
    EncodeKey {
        row: usize,
        col: u8,
    },
    /// Обслуживание разряда ВFD (такты 488..587).
    ServiceDigit {
        digit: usize,
    },
}

/// Псевдоадреса недокументированных состояний МК-52.
pub const ADDR_EGGOG: u16 = 0x6E00; // «Яйцо»: переполнение АЛУ / клавиша F1
pub const ADDR_DARKNESS: u16 = 0x6E10; // «Тьма»: гашение всего индикатора / F2
pub const ADDR_DUMMY: u16 = 0x6E20; // «Пустышка»: деление на ноль / F3
/// Служебные клавиши остановки и пуска программы.
pub const ADDR_PAUSE: u16 = 0x5031; // В/П (строка 3, столбец 1)
pub const ADDR_RUN_STOP: u16 = 0x5032; // С/П (строка 3, столбец 2)
/// Базовые адреса цифровых и функциональных клавиш.
pub const ADDR_DIGIT_BASE: u16 = 0x5E00;
pub const ADDR_F_BASE: u16 = 0x6E50;

/// Активна ли «Тьма» для данного разряда: при этом режиме ВПУ гасит все разряды,
/// кроме старшего (разряд 0). Используется и ядром, и GUI для корректной отрисовки.
pub fn darkness_blanks_digit(mode: Option<SpecialMode>, digit: usize) -> bool {
    matches!(mode, Some(SpecialMode::Darkness)) && digit != 0
}

#[derive(Clone, Debug)]
pub struct AuxProcessor {
    pub state: AuxState,
    /// Активное недокументированное состояние None -> нормальный режим.
    pub special_mode: Option<SpecialMode>,
    /// Запрошенная на текущем такте запись кода клавиши в PC ГП.
    pub key_press_target: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialMode {
    Eggog,
    Darkness,
    Dummy,
}

impl std::fmt::Display for SpecialMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpecialMode::Eggog => write!(f, "EGGOG"),
            SpecialMode::Darkness => write!(f, "Тьма"),
            SpecialMode::Dummy => write!(f, "Пустышка"),
        }
    }
}

impl Default for AuxProcessor {
    fn default() -> Self {
        Self {
            state: AuxState::Idle,
            special_mode: None,
            key_press_target: None,
        }
    }
}

impl AuxProcessor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Шаг FSM ВПУ на границе такта КС.
    pub fn tick(
        &mut self,
        clock: &ClockController,
        keyboard: &Keyboard,
        display: &mut VfdDisplay,
        regs: &RegisterFile,
    ) {
        self.key_press_target = None;
        if clock.in_keyboard_window() {
            let row = clock.tick; // одна строка за такт
            let mask = keyboard.scan_row(row);
            if let Some(col) = (0..5).find(|c| mask & (1 << c) != 0) {
                let col = col as u8;
                self.state = AuxState::EncodeKey { row, col };
                self.handle_key(row, col, regs);
                return;
            }
            self.state = AuxState::ScanKeyboard { row };
        } else if let Some(digit) = clock.active_digit() {
            self.state = AuxState::ServiceDigit { digit };
            // код разряда берётся из отдельного 10-словного окна отображения
            let code = regs.read_display_code(digit);
            display.service_digit(digit, code);
        } else {
            self.state = AuxState::Idle;
        }
    }

    /// Декодирование нажатия клавиши в адрес перехода микропрограммы.
    fn handle_key(&mut self, row: usize, col: u8, _regs: &RegisterFile) {
        let addr = key_to_addr(row, col);
        self.key_press_target = Some(addr);
        match addr {
            ADDR_EGGOG => self.special_mode = Some(SpecialMode::Eggog),
            ADDR_DARKNESS => self.special_mode = Some(SpecialMode::Darkness),
            ADDR_DUMMY => self.special_mode = Some(SpecialMode::Dummy),
            _ => {}
        }
    }

    /// Сброс недокументированного режима (по команде С/П или выключению питания).
    pub fn clear_special(&mut self) {
        self.special_mode = None;
    }
}

/// Таблица соответствия «строка x столбец» -> адрес входа микропрограммы.
/// Раскладка совпадает с `KEY_LABELS` в GUI: строки 0–1 — цифры 1..5 и 6..0,
/// строки 2–5 — операции и регистры, строки 6–9 — клавиши F1..F20.
pub fn key_to_addr(row: usize, col: u8) -> u16 {
    if row >= crate::peripheral::KB_ROWS || col >= crate::peripheral::KB_COLS as u8 {
        return 0;
    }
    let digit = match (row, col) {
        (0, 0) => Some(1),
        (0, 1) => Some(2),
        (0, 2) => Some(3),
        (0, 3) => Some(4),
        (0, 4) => Some(5),
        (1, 0) => Some(6),
        (1, 1) => Some(7),
        (1, 2) => Some(8),
        (1, 3) => Some(9),
        (1, 4) => Some(0),
        _ => None,
    };
    if let Some(d) = digit {
        return ADDR_DIGIT_BASE + (d as u16) * 0x10;
    }
    if row >= 6 {
        let index = (row - 6) * crate::peripheral::KB_COLS + col as usize;
        return match index {
            0 => ADDR_EGGOG,
            1 => ADDR_DARKNESS,
            2 => ADDR_DUMMY,
            _ => ADDR_F_BASE + ((index - 3) as u16) * 0x10,
        };
    }
    0x5000 + (row as u16) * 0x10 + col as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_follow_gui_layout() {
        assert_eq!(key_to_addr(0, 0), ADDR_DIGIT_BASE + 0x10);
        assert_eq!(key_to_addr(1, 3), ADDR_DIGIT_BASE + 0x90);
        assert_eq!(key_to_addr(1, 4), ADDR_DIGIT_BASE);
    }

    #[test]
    fn every_key_has_a_unique_address() {
        let mut seen = std::collections::HashSet::new();
        for row in 0..crate::peripheral::KB_ROWS {
            for col in 0..crate::peripheral::KB_COLS as u8 {
                assert!(
                    seen.insert(key_to_addr(row, col)),
                    "duplicate address at ({row}, {col})"
                );
            }
        }
        assert_eq!(
            seen.len(),
            crate::peripheral::KB_ROWS * crate::peripheral::KB_COLS
        );
    }

    #[test]
    fn run_stop_keys_have_service_addresses() {
        assert_eq!(key_to_addr(3, 1), ADDR_PAUSE);
        assert_eq!(key_to_addr(3, 2), ADDR_RUN_STOP);
    }
}
