//! Системный контроллер К745ИК1801 (КС): генератор 4-фазных тактов, счётчики циклов,
//! управление мультиплексированием ВFD и сканированием клавиатуры.
//!
//! По документации: опорная частота ~75 кГц; фазы M1..M4 следуют с шагом 1/300 мкс;
//! полный цикл сканирования периферии — 588 тактов (2352 импульса F1).

/// Полный цикл обслуживания периферии в тактах КС (каждый такт = фаза M1).
pub const CYCLE_TICKS: usize = 588;
/// Импульсы сдвига кольца за один такт КС.
pub const F1_PER_TICK: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    M1 = 0,
    M2 = 1,
    M3 = 2,
    M4 = 3,
}

impl Phase {
    pub fn next(self) -> Self {
        match self {
            Phase::M1 => Phase::M2,
            Phase::M2 => Phase::M3,
            Phase::M3 => Phase::M4,
            Phase::M4 => Phase::M1,
        }
    }
    pub fn index(self) -> usize {
        self as usize
    }
}

/// Состояние счётчиков КС на текущий момент времени.
#[derive(Clone, Debug)]
pub struct ClockController {
    /// Текущая фаза (M1..M4).
    pub phase: Phase,
    /// Номер такта внутри цикла 0..CYCLE_TICKS.
    pub tick: usize,
    /// Общий счётчик тактов (для трассировки).
    pub total_ticks: u64,
}

impl Default for ClockController {
    fn default() -> Self {
        Self::new()
    }
}

impl ClockController {
    pub fn new() -> Self {
        Self {
            phase: Phase::M1,
            tick: 0,
            total_ticks: 0,
        }
    }

    /// Продвинуть генератор на одну фазу. Возвращает true, если завершён полный такт (M4->M1).
    pub fn advance_phase(&mut self) -> bool {
        self.phase = self.phase.next();
        if self.phase == Phase::M1 {
            self.tick = (self.tick + 1) % CYCLE_TICKS;
            self.total_ticks += 1;
            true
        } else {
            false
        }
    }

    /// Окно сканирования клавиатуры: такты 0..=99 (10 строк по ~1 такту дешифратора).
    pub fn in_keyboard_window(&self) -> bool {
        self.tick < 100
    }

    /// Такт, на котором начинается обслуживание данного разряда индикатора (0..=9).
    /// Разряды опрашиваются в конце цикла: такты 488..=587, по 10 тактов на разряд.
    pub fn digit_service_tick(digit: usize) -> usize {
        488 + digit * 10
    }

    /// Активный разряд ВFD на текущем такте (None вне окна обслуживания).
    pub fn active_digit(&self) -> Option<usize> {
        if self.tick >= 488 {
            Some((self.tick - 488) / 10)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_sequence() {
        let mut c = ClockController::new();
        assert_eq!(c.phase, Phase::M1);
        c.advance_phase();
        assert_eq!(c.phase, Phase::M2);
        c.advance_phase();
        c.advance_phase();
        assert_eq!(c.phase, Phase::M4);
        assert!(c.advance_phase()); // M4->M1 = завершение такта
        assert_eq!(c.phase, Phase::M1);
    }

    #[test]
    fn digit_windows() {
        let mut c = ClockController::new();
        assert!(c.in_keyboard_window());
        c.tick = ClockController::digit_service_tick(3);
        assert_eq!(c.active_digit(), Some(3));
        c.tick = 200;
        assert_eq!(c.active_digit(), None);
        assert!(!c.in_keyboard_window());
    }
}
