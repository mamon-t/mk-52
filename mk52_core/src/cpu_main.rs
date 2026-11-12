//! Главный процессор (ГП, К145ИК1302): выборка команд из ПЗУ по адресу PC и выдача
//! управляющих сигналов. Реализован как классическая FSM: на каждом такте КС ГП
//! проходит состояния Fetch -> Decode -> Execute(обмен с кольцом) -> Next.

use crate::alu::{ArithmeticProcessor, MathOp};
use crate::clock::ClockController;
use crate::regfile::RegisterFile;
use crate::rom::{Opcode, Rom};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuState {
    Idle,      // выключено / дежурный режим («тьма» обрабатывается на уровне схемы питания)
    Fetch,     // выборка микрокоманды из ПЗУ по адресу PC (такт M1)
    Decode,    // разбор кода операции (M2)
    Execute,   // обмен с кольцом/АЛУ (M3)
    WriteBack, // завершение записи, инкремент PC (M4)
}

impl std::fmt::Display for CpuState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            CpuState::Idle => "Idle",
            CpuState::Fetch => "Fetch",
            CpuState::Decode => "Decode",
            CpuState::Execute => "Execute",
            CpuState::WriteBack => "WriteBack",
        };
        write!(f, "{s}")
    }
}

/// Сигналы, которые ГП выдаёт системному контроллеру в текущем такте.
#[derive(Clone, Copy, Debug, Default)]
pub struct BusRequest {
    pub need_ring_read: bool,
    pub need_ring_write: bool,
    /// Смещение слова в кольце, к которому обращается ГП.
    pub ring_offset: usize,
}

#[derive(Clone, Debug)]
pub struct MainProcessor {
    pub state: CpuState,
    /// Признак «работы калькулятора»: после В/П или С/П ГП крутит цикл программы.
    pub running: bool,
}

impl Default for MainProcessor {
    fn default() -> Self {
        Self {
            state: CpuState::Idle,
            running: false,
        }
    }
}

impl MainProcessor {
    /// Сброс при включении: PC := 000, состояние — ожидание команды КС.
    /// Сам PC/IR хранятся в кольце и сбрасываются вызывающей стороной.
    pub fn reset(&mut self) {
        self.state = CpuState::Fetch;
        self.running = true;
    }

    /// Шаг FSM главного процессора. Вызывается один раз за такт КС (фаза M1).
    /// Возвращает запрос на обслуживание шины в этом такте.
    pub fn step(
        &mut self,
        _clock: &ClockController,
        rom: &Rom,
        regs: &mut RegisterFile,
        alu: &mut ArithmeticProcessor,
    ) -> BusRequest {
        let mut req = BusRequest::default();
        if !self.running {
            self.state = CpuState::Idle;
            return req;
        }
        // одна команда за такт КС: выборка -> разбор -> исполнение -> инкремент PC
        match self.state {
            CpuState::Idle => {}
            _ => {
                let addr = self.fetch_addr(regs);
                let instruction = rom.read(addr);
                regs.write_ir(instruction);
                req.need_ring_read = true;
                req.ring_offset = crate::ring::word_to_offset(addr % 52);
                self.state = CpuState::Decode;

                let op = Opcode::from(instruction >> 4);
                req.need_ring_write = true;
                req.ring_offset = crate::ring::word_to_offset(self.field_word(instruction));
                let jumped = self.execute(op, instruction, regs, alu);
                self.state = CpuState::WriteBack;

                if !jumped {
                    self.advance_pc(regs);
                }
                self.state = CpuState::Fetch;
            }
        }
        req
    }

    /// Адресная часть выборки: PC определяет страницу ПЗУ, слово кольца — смещение.
    fn fetch_addr(&self, regs: &RegisterFile) -> usize {
        // упрощённая модель: страница = PC>>?; для эмуляции берём PC младшие 10 бит
        (regs.read_pc() as usize) & 0x3FF
    }

    /// Слово-операнд, на которое указывает поле адреса микрокоманды.
    fn field_word(&self, instruction: u8) -> usize {
        (instruction & 0x3F) as usize // 6-битное поле адреса регистра
    }

    /// Адрес перехода, упакованный в младшие 6 бит микрокоманды.
    fn jump_target(&self, instruction: u8) -> u16 {
        ((instruction as u16) << 4) & 0x3FF
    }

    /// Исполнение: пересылки между кольцевыми регистрами и запуск операций АЛУ.
    /// Возвращает true, если PC уже изменён командой перехода.
    fn execute(
        &mut self,
        op: Opcode,
        instruction: u8,
        regs: &mut RegisterFile,
        alu: &mut ArithmeticProcessor,
    ) -> bool {
        use Opcode::*;
        match op {
            Nop => {}
            Ab => {
                // AB: пересылка служебного регистра A в B.
                let a = regs.read_a();
                regs.write_b(a);
            }
            Px => {
                // PX: извлечение числа из регистра, на который указывает P.
                let p = regs.read_p();
                regs.write_x_u12(0, p);
            }
            Jx => {
                // JX: условный переход, если последний результат АЛУ отрицателен.
                if alu.y.negative {
                    regs.write_pc(self.jump_target(instruction) % 1000);
                    return true;
                }
            }
            Dx | Ex | Fx | Gx | Hx | Ix | Cx | Vx | Zx => {
                // пересылки регистров-счетов/адресов: моделируются копированием слова
                let w = self.field_word(instruction);
                let v = regs.read_x_u12(w);
                regs.write_y_u12(w, v);
            }
            Kx => {
                // KX: обмен X и Y
                let w = self.field_word(instruction);
                let a = regs.read_x_u12(w);
                let b = regs.read_y_u12(w);
                regs.write_x_u12(w, b);
                regs.write_y_u12(w, a);
            }
            S1 => {
                // +1 к слову в кольце (инкремент регистра X)
                let w = self.field_word(instruction);
                let v = regs.read_x_u12(w);
                regs.write_x_u12(w, v.wrapping_add(1) & 0xFFF);
            }
            S0 => {
                let w = self.field_word(instruction);
                let v = regs.read_x_u12(w);
                regs.write_x_u12(w, v.wrapping_sub(1) & 0xFFF);
            }
        }
        false
    }

    /// Инкремент PC с десятичным переносом (000..999). Вызывается только если
    /// команда не выполнила переход.
    fn advance_pc(&mut self, regs: &mut RegisterFile) {
        regs.write_pc((regs.read_pc() + 1) % 1000);
    }

    /// Явный запуск арифметической операции из микропрограммы (используется тестами).
    pub fn start_math(&self, regs: &RegisterFile, alu: &mut ArithmeticProcessor, op: MathOp) {
        let x = regs.x_number();
        let y = regs.y_number();
        alu.start(op, x, y);
    }
}
