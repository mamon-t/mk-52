//! Демонстрационный headless-прогон ядра эмулятора МК-52 (без GUI).
//! Тестирование на реальных программах выполняется после создания GUI-этапа.

use mk52_core::{BcdNumber, CoreEvent, Machine, MathOp};

fn main() {
    let mut m = Machine::new();
    m.handle_event(CoreEvent::PowerOn);
    println!("Включено. Индикатор: [{}]", m.display_snapshot());

    // демонстрация арифметики на уровне АЛУ (FSM-шаги): 1234 + 567890
    let x = BcdNumber::parse("1234").unwrap();
    let y = BcdNumber::parse("567890").unwrap();
    m.ap.start(MathOp::Add, x, y);
    for _ in 0..8 {
        m.tick();
        if m.ap.state == mk52_core::MathState::Idle {
            break;
        }
    }
    println!("Регистр Y после сложения: {:?}", m.ap.y);

    // прогон полных циклов КС (клавиатурное окно + обслуживание ВFD)
    m.run_cycles(2);
    println!(
        "Пройдено тактов: {}, отказов арбитража: {}",
        m.clock.total_ticks, m.memctl.denied_count
    );
}
