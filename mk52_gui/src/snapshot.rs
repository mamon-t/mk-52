//! Панель разработчика: форматирование снимка состояния ядра (`DebugSnapshot`)
//! в текстовые представления для окон отладки. Отделено от gpui, чтобы логику
//! можно было тестировать без графического окружения.

use mk52_core::DebugSnapshot;

/// Краткая сводка FSM и тактов (шапка панели).
pub fn summary(s: &DebugSnapshot) -> String {
    format!(
        "Тактов: {} | фаза M{} | PC {:03} | ГП {} | АЛУ {} | отказов шины: {}",
        s.total_ticks,
        s.phase.index() + 1,
        s.pc % 1000,
        s.gp_state,
        s.ap_state,
        s.denied_count,
    )
}

/// Таблица кольцевых регистров X/Y/слов знакомства.
pub fn registers_table(s: &DebugSnapshot) -> String {
    let mut out = String::new();
    out.push_str(&format!("X = {}\n", s.x_number.to_display_string()));
    out.push_str(&format!("Y = {}\n", s.y_number.to_display_string()));
    out.push_str(&format!(
        "Флаги X: знак={} порядок(код)={} | Флаги Y: знак={} порядок(код)={}\n",
        s.flag_x.sign, s.flag_x.exponent, s.flag_y.sign, s.flag_y.exponent
    ));
    out.push_str(&format!("Слово микрокоманды: {:02X}\n", s.instruction));
    out
}

/// Дамп пользовательской области памяти: WORDS_REG слов по 3 полубайта.
pub fn memory_dump(s: &DebugSnapshot) -> String {
    let mut out = String::new();
    for (i, w) in s.user_words.iter().enumerate() {
        if i % 8 == 0 {
            if i != 0 {
                out.push('\n');
            }
            out.push_str(&format!("{:03}: ", i));
        }
        out.push_str(&format!("[{:X}{:X}{:X}] ", w[2], w[1], w[0]));
    }
    out
}

/// Состояние ВFD: коды разрядов и активный разряд мультиплексирования.
pub fn vfd_table(s: &DebugSnapshot) -> String {
    let mut out = String::new();
    for (d, code) in s.vfd_codes.iter().enumerate() {
        let active = s.active_digit == Some(d);
        out.push_str(&format!(
            "разряд {}: код {:02X} '{}'{active}\n",
            d,
            code,
            mk52_core::VfdDisplay::decode(*code),
            active = if active { " ◀ активен" } else { "" }
        ));
    }
    if let Some(m) = &s.special_mode {
        out.push_str(&format!("Недокументированный режим: {m}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> DebugSnapshot {
        let m = mk52_core::Machine::new();
        m.debug_snapshot()
    }

    #[test]
    fn formats_without_panic() {
        let s = snapshot();
        assert!(summary(&s).contains("Тактов"));
        assert!(registers_table(&s).contains("X ="));
        assert!(memory_dump(&s).contains("000"));
        assert!(vfd_table(&s).contains("разряд 0"));
    }
}
