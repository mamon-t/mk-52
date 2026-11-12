//! Машина МК-52: интеграция КС, ГП, АЛУ, ВПУ, контроллера памяти, кольца, ПЗУ, EEPROM,
//! ВFD и клавиатуры. Цикло-точный планировщик: один вызов `tick()` = один такт КС
//! (4 импульса F1), все FSM продвигаются в порядке, соответствующем фазам M1..M4.

mod alu;
mod clock;
pub mod cpu_aux;
mod cpu_main;
mod eeprom;
mod memory_controller;
mod peripheral;
pub mod program;
mod regfile;
mod ring;
mod rom;

pub use alu::{ArithmeticProcessor, BcdNumber, DivError, MathFault, MathOp, MathState};
pub use clock::{CYCLE_TICKS, ClockController, F1_PER_TICK, Phase};
pub use cpu_aux::{
    ADDR_DARKNESS, ADDR_DUMMY, ADDR_EGGOG, ADDR_PAUSE, ADDR_RUN_STOP, AuxProcessor, AuxState,
    SpecialMode, darkness_blanks_digit, key_to_addr,
};
pub use cpu_main::{BusRequest, CpuState, MainProcessor};
pub use eeprom::{EEPROM_BYTES, Eeprom};
pub use memory_controller::{Agent, MemoryController};
pub use peripheral::{DigitCell, KB_COLS, KB_ROWS, Keyboard, VfdDisplay};
pub use program::{PROGRAM_CAPACITY, ProgramError, load_program, save_program};
pub use regfile::{RegisterFile, USER_REGS, WORDS_REG};
pub use ring::{DelayRing, FlagWord, RING_LEN, RingRegister};
pub use rom::{Opcode, ROM_WORDS, Rom};

/// Внешние события, доставляемые хостом (позднее — через mpsc-канал от GUI).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoreEvent {
    PowerOn,
    PowerOff,
    KeyPress {
        row: usize,
        col: usize,
    },
    KeyRelease {
        row: usize,
        col: usize,
    },
    /// Загрузка образа микропрограммы (для тестов без файла ROM).
    LoadRom(Vec<u8>),
    /// Загрузка пользовательской программы из текстового формата «МК-52 text»
    /// (см. модуль `program`). В ответ бэкенд шлёт `BackendMessage::ProgramLoaded`.
    LoadProgram(String),
    /// Выгрузить текущую пользовательскую программу в текст («МК-52 text»).
    SaveProgram,
    /// Пауза/возобновление исполнения (пошаговый режим обслуживает хост).
    SetPaused(bool),
}

/// Ответ машины хосту/GUI после обработки событий.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoreResponse {
    /// Программа загружена успешно: `n` слов размещено.
    ProgramLoaded(usize),
    /// Ошибка загрузки программы (текст ошибки для пользователя).
    ProgramError(String),
}

#[derive(Clone, Debug)]
pub struct Machine {
    pub clock: ClockController,
    pub gp: MainProcessor,
    pub ap: ArithmeticProcessor,
    pub vpu: AuxProcessor,
    pub memctl: MemoryController,
    pub regs: RegisterFile,
    pub rom: Rom,
    pub eeprom: Eeprom,
    pub display: VfdDisplay,
    pub keyboard: Keyboard,
    powered: bool,
    /// Пауза исполнения (пошаговый режим GUI): такты не продвигаются.
    paused: bool,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    pub fn new() -> Self {
        Self {
            clock: ClockController::new(),
            gp: MainProcessor::default(),
            ap: ArithmeticProcessor::default(),
            vpu: AuxProcessor::new(),
            memctl: MemoryController::new(),
            regs: RegisterFile::new(),
            rom: Rom::test_firmware(),
            eeprom: Eeprom::new(),
            display: VfdDisplay::new(),
            keyboard: Keyboard::new(),
            powered: false,
            paused: false,
        }
    }

    pub fn is_powered(&self) -> bool {
        self.powered
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Обработка внешнего события (аналог канала событий для будущего GUI).
    pub fn handle_event(&mut self, ev: CoreEvent) -> Option<CoreResponse> {
        match ev {
            CoreEvent::PowerOn => {
                if !self.powered {
                    self.powered = true;
                    self.regs.write_pc(0);
                    self.regs.write_ir(0);
                    self.gp.reset();
                    // восстановление из EEPROM выполняется первыми тактами микропрограммы
                }
                None
            }
            CoreEvent::PowerOff => {
                if self.powered {
                    self.save_state();
                    self.powered = false;
                    self.gp.running = false;
                    self.gp.state = CpuState::Idle;
                    self.vpu.clear_special();
                }
                None
            }
            CoreEvent::KeyPress { row, col } => {
                self.keyboard.press(row, col);
                None
            }
            CoreEvent::KeyRelease { row, col } => {
                self.keyboard.release(row, col);
                None
            }
            CoreEvent::LoadRom(data) => {
                self.rom = Rom::from_slice(&data);
                None
            }
            CoreEvent::LoadProgram(text) => {
                // загрузка выполняется в кольцевые регистры пользовательской области
                match program::load_program(&text, &mut self.regs) {
                    Ok(n) => Some(CoreResponse::ProgramLoaded(n)),
                    Err(e) => Some(CoreResponse::ProgramError(e.to_string())),
                }
            }
            CoreEvent::SaveProgram => None, // ответ формируется хостом через program_dump()
            CoreEvent::SetPaused(p) => {
                self.paused = p;
                None
            }
        }
    }

    /// Выгрузка текущей пользовательской программы в текст «МК-52 text» (для GUI).
    pub fn program_dump(&self) -> String {
        program::save_program(&self.regs)
    }

    /// Загрузка образа ПЗУ с валидацией размера.
    pub fn load_rom_file(&mut self, path: &str) -> std::io::Result<()> {
        self.rom = Rom::load_file(path)?;
        Ok(())
    }

    /// Сохранение EEPROM в файл (512 байт).
    pub fn save_eeprom_file(&mut self, path: &str) -> std::io::Result<()> {
        self.save_state();
        self.eeprom.save_file(path)
    }

    /// Загрузка EEPROM из файла и восстановление пользовательских регистров.
    pub fn load_eeprom_file(&mut self, path: &str) -> std::io::Result<()> {
        self.eeprom = Eeprom::load_file(path)?;
        self.eeprom.restore_into(&mut self.regs);
        Ok(())
    }

    /// Сохранение кольцевых регистров в EEPROM перед выключением.
    fn save_state(&mut self) {
        for w in 0..regfile::WORDS_REG {
            let [l, m, h] = self.regs.read_user_area_word(w);
            let base = w * 3;
            self.eeprom.write_word(base, l);
            self.eeprom.write_word(base + 1, m);
            self.eeprom.write_word(base + 2, h);
        }
    }

    /// Один такт КС: четыре фазы F1/M1..M4, кольцо сдвигается на 4 позиции.
    ///
    /// Единственный источник времени — [`ClockController`]. Счётчики `tick` и
    /// `total_ticks` инкрементируются только в `advance_phase()` при переходе
    /// M4 -> M1, поэтому один вызов `tick()` ровно соответствует одному такту КС.
    pub fn tick(&mut self) -> u64 {
        if !self.powered || self.paused {
            return self.clock.total_ticks;
        }

        for _ in 0..F1_PER_TICK {
            match self.clock.phase {
                Phase::M1 => {
                    // выборка/исполнение команды ГП и запрос к шине
                    let gp_req = self
                        .gp
                        .step(&self.clock, &self.rom, &mut self.regs, &mut self.ap);
                    self.memctl.phase_hook(&self.clock, &gp_req);

                    // применение адреса клавиши, сформированного ВПУ на предыдущем такте
                    if let Some(addr) = self.vpu.key_press_target.take() {
                        match addr {
                            ADDR_PAUSE => {
                                self.gp.running = false;
                                self.gp.state = CpuState::Idle;
                            }
                            ADDR_RUN_STOP => {
                                if self.gp.running {
                                    self.gp.running = false;
                                    self.gp.state = CpuState::Idle;
                                } else {
                                    self.gp.running = true;
                                    self.gp.state = CpuState::Fetch;
                                    self.vpu.clear_special();
                                }
                            }
                            _ => {
                                self.regs.write_pc(addr % 1000);
                                self.gp.state = CpuState::Fetch;
                            }
                        }
                    }
                }
                Phase::M3 => {
                    // АЛУ: последовательная обработка полубайтов
                    self.ap.step();
                    if self.ap.state == MathState::Done {
                        self.regs.set_y_number(&self.ap.y);
                        match self.ap.fault {
                            Some(MathFault::Overflow) => {
                                self.vpu.special_mode = Some(SpecialMode::Eggog);
                            }
                            Some(MathFault::DivideByZero) => {
                                self.vpu.special_mode = Some(SpecialMode::Dummy);
                            }
                            None => {}
                        }
                        self.ap.reset();
                    }
                }
                Phase::M4 => {
                    // периферия ВПУ и завершение арбитража шины
                    self.vpu
                        .tick(&self.clock, &self.keyboard, &mut self.display, &self.regs);
                    self.memctl.end_tick();
                }
                Phase::M2 => {}
            }

            // кольцо сдвигается на каждом импульсе F1
            self.regs.ring.shift();
            self.clock.advance_phase();
        }

        self.clock.total_ticks
    }

    /// Продвинуть машину на `n` полных циклов КС (588 тактов каждый).
    pub fn run_cycles(&mut self, n: usize) {
        for _ in 0..n {
            for _ in 0..CYCLE_TICKS {
                self.tick();
            }
        }
    }

    /// Снимок состояния индикатора для хоста/GUI.
    pub fn display_snapshot(&self) -> String {
        self.display.render()
    }

    /// Полный снимок внутреннего состояния машины для панели разработчика (см. gui/).
    pub fn debug_snapshot(&self) -> DebugSnapshot {
        let mut ring_bytes = [0u8; RING_LEN / 2];
        for (i, byte) in ring_bytes.iter_mut().enumerate() {
            *byte =
                (self.regs.ring.read_pos(2 * i) & 0x0F) | (self.regs.ring.read_pos(2 * i + 1) << 4);
        }
        DebugSnapshot {
            total_ticks: self.clock.total_ticks,
            tick_in_cycle: self.clock.tick,
            phase: self.clock.phase,
            powered: self.powered,
            gp_state: self.gp.state,
            pc: self.regs.read_pc(),
            instruction: self.regs.read_ir(),
            ap_state: self.ap.state,
            x_number: self.regs.x_number(),
            y_number: self.regs.y_number(),
            flag_x: self.regs.x_flag(),
            flag_y: self.regs.y_flag(),
            user_words: (0..regfile::WORDS_REG)
                .map(|w| self.regs.read_user_area_word(w))
                .collect(),
            vfd_codes: self.display.digits.iter().map(|d| d.code).collect(),
            active_digit: self.clock.active_digit(),
            special_mode: self.vpu.special_mode.map(|m| m.to_string()),
            denied_count: self.memctl.denied_count,
            head: self.regs.ring.head(),
            ring_bytes,
        }
    }
}

/// Сериализуемый снимок всех наблюдаемых состояний ядра — контракт между эмулятором
/// и панелью разработчика GUI (обновляется раз в кадр, см. gui/src/snapshot.rs).
#[derive(Clone, Debug)]
pub struct DebugSnapshot {
    pub total_ticks: u64,
    pub tick_in_cycle: usize,
    pub phase: Phase,
    pub powered: bool,
    pub gp_state: CpuState,
    pub pc: u16,
    pub instruction: u8,
    pub ap_state: MathState,
    pub x_number: BcdNumber,
    pub y_number: BcdNumber,
    pub flag_x: FlagWord,
    pub flag_y: FlagWord,
    /// Содержимое пользовательской области кольца: WORDS_REG слов по 3 полубайта.
    pub user_words: Vec<[u8; 3]>,
    /// Коды разрядов ВFD (10 шт., таблица К745ГФ3).
    pub vfd_codes: Vec<u8>,
    pub active_digit: Option<usize>,
    pub special_mode: Option<String>,
    pub denied_count: u64,
    pub head: usize,
    /// Всё кольцо задержки: 1176 байт (по два полубайта в байте).
    pub ring_bytes: [u8; RING_LEN / 2],
}
