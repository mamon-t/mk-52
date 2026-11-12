//! Фоновый поток эмуляции (бэкенд) и мост «ядро ↔ GUI».
//!
//! Архитектура (по утверждённому плану):
//!  - `Machine` живёт в собственном потоке, крутится цикло-точно (~75 кГц) либо
//!    без ограничений в турбо-режиме;
//!  - GUI шлёт события ядра (`CoreEvent`) и управляющие команды (`ControlMsg`)
//!    через mpsc-каналы;
//!  - бэкенд публикует снимки состояния в слот `Arc<Mutex<Option<T>>>` — UI
//!    забирает самый свежий неблокирующе (backpressure: старые кадры просто
//!    затираются новыми);
//!  - результаты выгрузки программ возвращаются встречным каналом
//!    `BackendMessage`.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use mk52_core::{CoreEvent, CoreResponse, DebugSnapshot, Machine};

/// Управление потоком из GUI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlMsg {
    SetTurbo(bool),
    /// Пауза/снятие паузы (реализуется через `CoreEvent::SetPaused`).
    SetPaused(bool),
    /// Выполнить ровно один цикл КС (пошаговый режим).
    StepOnce,
    Shutdown,
}

/// Ответы бэкенда в сторону GUI.
#[derive(Clone, Debug)]
pub enum BackendMessage {
    /// Результат `CoreEvent::SaveProgram` — текст «МК-52 text».
    Saved(String),
    /// Программа загружена: `n` слов размещено.
    Loaded(usize),
    /// Ошибка загрузки программы.
    LoadError(String),
}

/// Слот последнего снимка: пишется бэкендом, читается UI-тредом.
pub type SnapshotSlot = Arc<Mutex<Option<DebugSnapshot>>>;
/// Слот ответов бэкенда (программы и пр.).
pub type MessageSlot = Arc<Mutex<Option<BackendMessage>>>;

/// Клиентская сторона бэкенда: живёт в gpui-сущности, ничего не блокирует.
pub struct EmuBackend {
    events: Sender<CoreEvent>,
    control: Sender<ControlMsg>,
    snapshots: SnapshotSlot,
    messages: MessageSlot,
    handle: Option<JoinHandle<()>>,
    turbo: bool,
    paused: bool,
    powered: bool,
}

impl EmuBackend {
    /// Запуск потока эмуляции.
    pub fn spawn() -> Self {
        let (ev_tx, ev_rx) = mpsc::channel::<CoreEvent>();
        let (ctl_tx, ctl_rx) = mpsc::channel::<ControlMsg>();
        let snapshots: SnapshotSlot = Arc::new(Mutex::new(None));
        let messages: MessageSlot = Arc::new(Mutex::new(None));
        let snap_pub = snapshots.clone();
        let msg_pub = messages.clone();
        let handle = thread::spawn(move || backend_loop(ev_rx, ctl_rx, snap_pub, msg_pub));
        Self {
            events: ev_tx,
            control: ctl_tx,
            snapshots,
            messages,
            handle: Some(handle),
            turbo: false,
            paused: false,
            powered: false,
        }
    }

    pub fn send_event(&self, ev: CoreEvent) {
        let _ = self.events.send(ev);
    }

    pub fn send_control(&self, msg: ControlMsg) {
        let _ = self.control.send(msg);
    }

    // ---- API для view'ов (неблокирующие) ----------------------------------

    pub fn key_press(&self, row: usize, col: usize) {
        self.send_event(CoreEvent::KeyPress { row, col });
    }

    pub fn key_release(&self, row: usize, col: usize) {
        self.send_event(CoreEvent::KeyRelease { row, col });
    }

    pub fn toggle_power(&mut self) {
        if self.powered {
            self.send_event(CoreEvent::PowerOff);
            self.powered = false;
        } else {
            self.send_event(CoreEvent::PowerOn);
            self.powered = true;
        }
    }

    pub fn set_turbo(&mut self, on: bool) {
        self.turbo = on;
        self.send_control(ControlMsg::SetTurbo(on));
    }

    pub fn step_once(&self) {
        self.send_control(ControlMsg::StepOnce);
    }

    pub fn set_paused(&mut self, on: bool) {
        self.paused = on;
        self.send_control(ControlMsg::SetPaused(on));
    }

    /// Загрузить программу «МК-52 text» в ядро.
    pub fn load_program(&self, text: String) {
        self.send_event(CoreEvent::LoadProgram(text));
    }

    /// Запросить выгрузку текущей программы (ответ — через `poll_message`).
    pub fn request_save_program(&self) {
        self.send_event(CoreEvent::SaveProgram);
    }

    /// Забрать самый свежий снимок, затёртый с момента прошлого опроса.
    pub fn poll_snapshot(&self) -> Option<DebugSnapshot> {
        self.snapshots.lock().ok()?.take()
    }

    /// Забрать самое свежее сообщение бэкенда.
    pub fn poll_message(&self) -> Option<BackendMessage> {
        self.messages.lock().ok()?.take()
    }
}

impl Drop for EmuBackend {
    fn drop(&mut self) {
        self.send_control(ControlMsg::Shutdown);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl std::fmt::Debug for EmuBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmuBackend")
            .field("turbo", &self.turbo)
            .field("powered", &self.powered)
            .finish_non_exhaustive()
    }
}

/// Опубликовать текущее состояние машины в слот для UI.
fn publish_snapshot(machine: &Machine, slot: &SnapshotSlot) {
    if let Ok(mut snapshot) = slot.lock() {
        *snapshot = Some(machine.debug_snapshot());
    }
}

/// Цикл бэкенда: один цикл КС (588 тактов → кадр ВFD) за итерацию;
/// снимок публикуется на границе цикла.
fn backend_loop(
    ev_rx: Receiver<CoreEvent>,
    ctl_rx: Receiver<ControlMsg>,
    snap_slot: SnapshotSlot,
    msg_slot: MessageSlot,
) {
    let mut machine = Machine::new();
    let mut turbo = false;
    let mut paused = false;
    let mut step_pending = false;
    let mut dirty = true;
    // Реалистичный темп: кадр индикатора ~ 1/128 c (цикл мультиплексирования ВFD).
    let frame_period = std::time::Duration::from_micros(1_000_000 / 128);
    let mut last_frame_cycle = machine.clock.total_ticks / 588;

    loop {
        let start = std::time::Instant::now();

        while let Ok(ev) = ev_rx.try_recv() {
            dirty = true;
            match ev {
                CoreEvent::SaveProgram => {
                    let text = machine.program_dump();
                    if let Ok(mut slot) = msg_slot.lock() {
                        *slot = Some(BackendMessage::Saved(text));
                    }
                }
                other => {
                    if let Some(resp) = machine.handle_event(other)
                        && let Ok(mut slot) = msg_slot.lock()
                    {
                        *slot = Some(match resp {
                            CoreResponse::ProgramLoaded(n) => BackendMessage::Loaded(n),
                            CoreResponse::ProgramError(e) => BackendMessage::LoadError(e),
                        });
                    }
                }
            }
        }
        while let Ok(msg) = ctl_rx.try_recv() {
            dirty = true;
            match msg {
                ControlMsg::SetTurbo(t) => {
                    turbo = t;
                    if t {
                        paused = false;
                        machine.handle_event(CoreEvent::SetPaused(false));
                    }
                }
                ControlMsg::SetPaused(p) => {
                    paused = p;
                    machine.handle_event(CoreEvent::SetPaused(p));
                }
                ControlMsg::StepOnce => step_pending = true,
                ControlMsg::Shutdown => return,
            }
        }

        // Пауза не должна крутить поток вхолостую. Один шаг выполняется
        // временным снятием паузы, после чего состояние восстанавливается.
        if paused && !step_pending {
            if dirty {
                publish_snapshot(&machine, &snap_slot);
                dirty = false;
            }
            thread::sleep(frame_period);
            continue;
        }

        let stepped_while_paused = paused && step_pending;
        step_pending = false;
        if stepped_while_paused {
            machine.handle_event(CoreEvent::SetPaused(false));
        }

        machine.run_cycles(1);

        if stepped_while_paused {
            machine.handle_event(CoreEvent::SetPaused(true));
        }

        let frame = machine.clock.total_ticks / 588;
        if dirty || frame != last_frame_cycle {
            last_frame_cycle = frame;
            publish_snapshot(&machine, &snap_slot);
            dirty = false;
        }

        if !turbo && let Some(sleep) = frame_period.checked_sub(start.elapsed()) {
            thread::sleep(sleep);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn backend_publishes_snapshots() {
        let backend = EmuBackend::spawn();
        backend.send_event(CoreEvent::PowerOn);
        // ждём снимка уже включённой машины (первый снимок может быть выключенным)
        let mut snap = None;
        for _ in 0..200 {
            if let Some(s) = backend.poll_snapshot()
                && s.powered
            {
                snap = Some(s);
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        let snap = snap.expect("бэкенд должен опубликовать снимок включённой машины");
        assert!(snap.powered);
    }

    #[test]
    fn save_program_roundtrip() {
        let backend = EmuBackend::spawn();
        backend.send_event(CoreEvent::PowerOn);
        // Останавливаем исполнение, чтобы ГП не успел изменить область программы
        // между загрузкой и выгрузкой.
        backend.send_event(CoreEvent::SetPaused(true));
        backend.load_program("000 /АВ\n001 /В/П\n002 /КХ".into());

        let mut loaded = false;
        for _ in 0..400 {
            match backend.poll_message() {
                Some(BackendMessage::Loaded(3)) => {
                    loaded = true;
                    break;
                }
                Some(BackendMessage::LoadError(e)) => panic!("ошибка загрузки: {e}"),
                _ => thread::sleep(Duration::from_millis(10)),
            }
        }
        assert!(loaded, "ожидался ответ Loaded(3)");

        backend.request_save_program();
        let mut got = None;
        for _ in 0..400 {
            if let Some(BackendMessage::Saved(text)) = backend.poll_message() {
                got = Some(text);
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let text = got.expect("ожидался ответ Saved");
        assert_eq!(
            text.lines().count(),
            mk52_core::PROGRAM_CAPACITY,
            "неверный дамп: {text:?}"
        );
        assert!(text.starts_with("000 220\n"), "неверный дамп: {text:?}");
    }
}
