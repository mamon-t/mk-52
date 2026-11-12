//! Контроллер памяти (К145ИК1306): арбитр доступа к кольцевой шине между
//! главным, арифметическим и вспомогательным процессорами.
//!
//! По документации: каждый из трёх процессоров обращается к кольцу «своим третьим
//! битом» (R1 — позиции 3k+0, R2 — 3k+1, R3 — 3k+2), поэтому конфликты возможны
//! только внутри одного слота; контроллер выдаёт разрешения по приоритету
//! ГП > АЛУ > ВПУ в пределах такта.

use crate::clock::{ClockController, Phase};
use crate::cpu_main::BusRequest;
use crate::regfile::RegisterFile;

/// Приоритеты агентов шины.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Agent {
    Gp,  // главный процессор (К145ИК1302)
    Ap,  // арифметический процессор (К145ИК1303)
    Vpu, // вспомогательный процессор (К145ИК1306, ввод-вывод)
}

#[derive(Clone, Debug, Default)]
pub struct MemoryController {
    /// Слоты, занятые агентами в текущем такте.
    pub granted: Option<(Agent, usize)>,
    /// Счётчик отказов арбитрежа (для трассировки).
    pub denied_count: u64,
}

impl MemoryController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Арбитраж запроса на доступ к кольцу. Возвращает true, если разрешение выдано.
    pub fn request(&mut self, agent: Agent, offset: usize) -> bool {
        match self.granted {
            None => {
                self.granted = Some((agent, offset));
                true
            }
            Some((cur, _)) => {
                let win = matches!(
                    (cur, agent),
                    (Agent::Vpu, Agent::Gp) | (Agent::Vpu, Agent::Ap) | (Agent::Ap, Agent::Gp)
                );
                if win {
                    self.granted = Some((agent, offset));
                    true
                } else {
                    self.denied_count += 1;
                    false
                }
            }
        }
    }

    /// Исполнение разрешённого обращения: чтение слова из единого кольца.
    pub fn execute_read(&self, regs: &RegisterFile, offset: usize) -> [u8; 3] {
        regs.read_bus_word(offset)
    }

    /// Исполнение разрешённой записи слова в единое кольцо.
    pub fn execute_write(&self, regs: &mut RegisterFile, offset: usize, value: [u8; 3]) {
        regs.write_bus_word(offset, value);
    }

    /// Завершение такта: снятие всех разрешений (на фазе M4).
    pub fn end_tick(&mut self) {
        self.granted = None;
    }

    /// Тактовый проход: вызывается системным контроллером на каждой фазе.
    /// На M1 обслуживаются запросы ГП, на M2/M3 — АЛУ, на M4 — ВПУ + сброс.
    pub fn phase_hook(&mut self, clock: &ClockController, gp_req: &BusRequest) {
        match clock.phase {
            Phase::M1 => {
                if gp_req.need_ring_read || gp_req.need_ring_write {
                    self.request(Agent::Gp, gp_req.ring_offset);
                }
            }
            Phase::M4 => {
                self.end_tick();
            }
            _ => {}
        }
    }
}
