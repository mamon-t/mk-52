//! Интеграционные тесты машины МК-52 (без GUI — по утверждённому плану).

use mk52_core::*;

#[test]
fn power_on_starts_clock_and_cpu() {
    let mut m = Machine::new();
    assert!(!m.is_powered());
    m.handle_event(CoreEvent::PowerOn);
    assert!(m.is_powered());
    m.tick();
    assert_eq!(m.clock.total_ticks, 1);
    assert_eq!(m.gp.state, CpuState::Fetch);
}

#[test]
fn ring_shifts_four_positions_per_tick() {
    let mut m = Machine::new();
    m.handle_event(CoreEvent::PowerOn);
    let h0 = m.regs.ring.head();
    for _ in 0..10 {
        m.tick();
    }
    assert_eq!(m.regs.ring.head(), (h0 + 40) % RING_LEN);
}

#[test]
fn clock_has_single_source_of_time() {
    let mut m = Machine::new();
    m.handle_event(CoreEvent::PowerOn);
    let h0 = m.regs.ring.head();

    for n in 1..=4 {
        m.tick();
        assert_eq!(m.clock.total_ticks, n);
        assert_eq!(m.clock.tick, n as usize);
        assert_eq!(m.clock.phase, Phase::M1);
    }

    for n in 5..=CYCLE_TICKS {
        m.tick();
        assert_eq!(m.clock.total_ticks, n as u64);
        assert_eq!(m.clock.tick, n % CYCLE_TICKS);
        assert_eq!(m.clock.phase, Phase::M1);
    }
    assert_eq!(
        m.regs.ring.head(),
        h0,
        "полный цикл возвращает голову кольца"
    );
}

#[test]
fn keyboard_press_sets_pc_via_aux() {
    let mut m = Machine::new();
    m.handle_event(CoreEvent::PowerOn);
    // нажатие цифры «3» (строка 0, столбец 2)
    m.keyboard.press(0, 2);
    // прогон до окна сканирования клавиатуры (такты 0..99 цикла КС)
    for _ in 0..CYCLE_TICKS {
        m.tick();
        if m.regs.read_pc() == key_to_addr(0, 2) % 1000 {
            break;
        }
    }
    let expected = key_to_addr(0, 2) % 1000;
    assert_eq!(
        m.regs.read_pc(),
        expected,
        "PC должен получить адрес обработки клавиши"
    );
}

#[test]
fn vfd_digit_service_populates_display() {
    let mut m = Machine::new();
    m.handle_event(CoreEvent::PowerOn);
    m.regs.write_display_code(0, 0x3); // код символа '2' для разряда 0
    for _ in 0..CYCLE_TICKS {
        m.tick();
    }
    let snap = m.display_snapshot();
    assert!(
        snap.contains('2'),
        "индикатор должен отобразить обслуженный разряд: [{snap}]"
    );
}

#[test]
fn px_recalls_p_register() {
    let mut m = Machine::new();
    m.handle_event(CoreEvent::PowerOn);
    m.handle_event(CoreEvent::LoadRom(vec![0x30]));
    m.regs.write_p(0x456);
    m.tick();
    assert_eq!(m.regs.read_x_u12(0), 0x456);
}

#[test]
fn alu_overflow_and_division_by_zero_set_special_modes() {
    let mut m = Machine::new();
    m.handle_event(CoreEvent::PowerOn);
    m.ap.start(
        MathOp::Div,
        BcdNumber::parse("1").unwrap(),
        BcdNumber::zero(),
    );
    for _ in 0..8 {
        m.tick();
        if m.ap.state == MathState::Idle {
            break;
        }
    }
    assert_eq!(m.vpu.special_mode, Some(SpecialMode::Dummy));

    m.handle_event(CoreEvent::PowerOff);
    m.handle_event(CoreEvent::PowerOn);
    let x = BcdNumber::parse("11111111").unwrap();
    let mut y = BcdNumber::parse("9").unwrap();
    y.exponent = 99;
    m.ap.start(MathOp::Mul, x, y);
    for _ in 0..8 {
        m.tick();
        if m.ap.state == MathState::Idle {
            break;
        }
    }
    assert_eq!(m.vpu.special_mode, Some(SpecialMode::Eggog));
}

#[test]
fn run_stop_key_toggles_cpu() {
    let mut m = Machine::new();
    m.handle_event(CoreEvent::PowerOn);
    m.keyboard.press(3, 2);
    for _ in 0..CYCLE_TICKS {
        m.tick();
        if !m.gp.running {
            break;
        }
    }
    assert!(!m.gp.running, "С/П должен остановить программу");
    m.keyboard.release(3, 2);
    for _ in 0..CYCLE_TICKS {
        m.tick();
    }
    m.keyboard.press(3, 2);
    for _ in 0..CYCLE_TICKS {
        m.tick();
        if m.gp.running {
            break;
        }
    }
    assert!(m.gp.running, "повторное С/П должно запустить программу");
}

#[test]
fn alu_fsm_add_through_machine() {
    let mut m = Machine::new();
    m.handle_event(CoreEvent::PowerOn);
    let x = BcdNumber::parse("1234").unwrap();
    let y = BcdNumber::parse("567890").unwrap();
    m.ap.start(MathOp::Add, x, y);
    let mut steps = 0;
    while m.ap.state != MathState::Idle && steps < 16 {
        m.tick();
        steps += 1;
    }
    assert!(
        (1..=4).contains(&steps),
        "операция должна завершиться за конечное число тактов FSM: {steps}"
    );
    // 1234 + 567890 = 569124
    let digits: Vec<u8> = m.ap.y.digits.to_vec();
    assert_eq!(digits[..6], [5, 6, 9, 1, 2, 4]);
}

#[test]
fn eeprom_save_restore_roundtrip() {
    let mut m = Machine::new();
    m.handle_event(CoreEvent::PowerOn);
    assert!(m.regs.write_user_word(0, 0, [1, 2, 3]));
    m.handle_event(CoreEvent::PowerOff);
    assert_eq!(m.eeprom.read_word(0), 1);
    m.handle_event(CoreEvent::PowerOn);
    m.eeprom.restore_into(&mut m.regs);
    assert_eq!(m.regs.read_user_word(0, 0), Some([1, 2, 3]));
}

#[test]
fn arbiter_priority_gp_over_vpu() {
    let mut mc = MemoryController::new();
    assert!(mc.request(Agent::Vpu, 0));
    assert!(mc.request(Agent::Gp, 3));
    assert!(!mc.request(Agent::Ap, 6));
    assert_eq!(mc.denied_count, 1);
    mc.end_tick();
    assert!(mc.granted.is_none());
}
