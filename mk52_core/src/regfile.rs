//! Регистровый файл МК-52, распределённый по кольцевой шине.
//!
//! По документации К145ИК130x: X и Y (мантиссы + слова знакомства), A, B, C, P,
//! R1/R2/R3, счётчик программ PC, регистр команд IR — все хранятся полубайтами
//! в кольце; доступ «каждым третьим битом» задаётся базовым смещением регистра.
//!
//! В этой реализации данные не дублируются: [`RingRegister`] — только описание
//! окна, а единственным хранилищем остаётся [`DelayRing`].

use crate::alu::BcdNumber;
use crate::ring::{DelayRing, FlagWord, RingRegister};

/// Количество пользовательских регистров (00..14).
pub const USER_REGS: usize = 15;
/// Слов данных в пользовательской области: 15 регистров по 3 слова.
pub const WORDS_REG: usize = USER_REGS * 3;

/// Базовые смещения кольцевых регистров (в позициях кольца).
/// Используются как документированная карта регистра файла К145ИК1302;
/// доступ к X/Y/пользовательской области идёт через окна [`RingRegister`].
#[allow(dead_code)]
pub mod base {
    pub const X: usize = 0; // регистр X: 3 слова
    pub const Y: usize = 9; // регистр Y: 3 слова
    pub const A: usize = 18; // слово знакомства A
    pub const B: usize = 27; // слово знакомства B
    pub const C: usize = 36; // адресный регистр C
    pub const P: usize = 45; // регистр P
    pub const PC: usize = 54; // счётчик программ
    pub const IR: usize = 63; // регистр команд
    pub const USER: usize = 100; // пользовательские регистры 00..14
    pub const DISPLAY: usize = 240; // окно из 10 слов для кодов ВФД
}

/// Количество разрядов ВФД (окно отображения в кольце).
pub const DISPLAY_DIGITS: usize = 10;

#[derive(Clone, Debug)]
pub struct RegisterFile {
    /// Единственное хранилище регистров.
    pub ring: DelayRing,
    /// Окно X (3 слова, слово 2 — слово знакомства).
    pub x: RingRegister,
    /// Окно Y (3 слова, слово 2 — слово знакомства).
    pub y: RingRegister,
    /// Пользовательская область: 15 регистров по 3 слова.
    pub user: RingRegister,
}

impl Default for RegisterFile {
    fn default() -> Self {
        Self::new()
    }
}

impl RegisterFile {
    pub fn new() -> Self {
        Self {
            ring: DelayRing::new(),
            x: RingRegister::new(base::X, 3),
            y: RingRegister::new(base::Y, 3),
            user: RingRegister::new(base::USER, WORDS_REG),
        }
    }

    pub fn read_x_u12(&self, word: usize) -> u16 {
        self.x.read_u12(&self.ring, word)
    }
    pub fn write_x_u12(&mut self, word: usize, v: u16) {
        self.x.write_u12(&mut self.ring, word, v);
    }
    pub fn read_y_u12(&self, word: usize) -> u16 {
        self.y.read_u12(&self.ring, word)
    }
    pub fn write_y_u12(&mut self, word: usize, v: u16) {
        self.y.write_u12(&mut self.ring, word, v);
    }

    pub fn read_x_h(&self, word: usize) -> u8 {
        self.x.read_h(&self.ring, word)
    }

    pub fn write_x_h(&mut self, word: usize, value: u8) {
        self.x.write_h(&mut self.ring, word, value);
    }

    /// Счётчик программ: одно 12-битное слово в кольце.
    pub fn read_pc(&self) -> u16 {
        RingRegister::new(base::PC, 1).read_u12(&self.ring, 0)
    }

    pub fn write_pc(&mut self, value: u16) {
        RingRegister::new(base::PC, 1).write_u12(&mut self.ring, 0, value & 0x0FFF);
    }

    /// Регистр команд: младшие 8 бит 12-битного слова кольца.
    pub fn read_ir(&self) -> u8 {
        RingRegister::new(base::IR, 1).read_u12(&self.ring, 0) as u8
    }

    pub fn write_ir(&mut self, value: u8) {
        RingRegister::new(base::IR, 1).write_u12(&mut self.ring, 0, value as u16);
    }

    /// Служебные регистры A, B и P (по одному 12-битному слову).
    pub fn read_a(&self) -> u16 {
        RingRegister::new(base::A, 1).read_u12(&self.ring, 0)
    }

    pub fn write_a(&mut self, value: u16) {
        RingRegister::new(base::A, 1).write_u12(&mut self.ring, 0, value & 0x0FFF);
    }

    pub fn read_b(&self) -> u16 {
        RingRegister::new(base::B, 1).read_u12(&self.ring, 0)
    }

    pub fn write_b(&mut self, value: u16) {
        RingRegister::new(base::B, 1).write_u12(&mut self.ring, 0, value & 0x0FFF);
    }

    pub fn read_p(&self) -> u16 {
        RingRegister::new(base::P, 1).read_u12(&self.ring, 0)
    }

    pub fn write_p(&mut self, value: u16) {
        RingRegister::new(base::P, 1).write_u12(&mut self.ring, 0, value & 0x0FFF);
    }

    /// Код разряда ВФД хранится в H-полубайте отдельного слова кольца.
    pub fn read_display_code(&self, digit: usize) -> u8 {
        if digit >= DISPLAY_DIGITS {
            return 0;
        }
        RingRegister::new(base::DISPLAY, DISPLAY_DIGITS).read_h(&self.ring, digit)
    }

    pub fn write_display_code(&mut self, digit: usize, code: u8) {
        if digit < DISPLAY_DIGITS {
            RingRegister::new(base::DISPLAY, DISPLAY_DIGITS).write_h(
                &mut self.ring,
                digit,
                code & 0x0F,
            );
        }
    }

    /// Чтение числа из регистра X как BCD (упрощённо: цифры хранятся в словах 0..2).
    pub fn x_number(&self) -> BcdNumber {
        Self::ring_to_bcd(&self.ring, &self.x)
    }
    pub fn y_number(&self) -> BcdNumber {
        Self::ring_to_bcd(&self.ring, &self.y)
    }

    pub fn x_flag(&self) -> FlagWord {
        self.x.flag_word(&self.ring, Self::FLAG_SLOT)
    }

    pub fn y_flag(&self) -> FlagWord {
        self.y.flag_word(&self.ring, Self::FLAG_SLOT)
    }

    /// Слово знакомства хранится в слове 2 регистра (K145ИК130x):
    /// L — знак (1 = отрицательное), H — порядок (BCD 0..99), M — служебное поле.
    const FLAG_SLOT: usize = 2;

    fn ring_to_bcd(ring: &DelayRing, r: &RingRegister) -> BcdNumber {
        // 8 цифр мантиссы раскладываем по словам: слово0=L,M,H -> цифры 0..2 и т.д.
        let mut digits = [0u8; 8];
        let mut di = 0;
        for w in 0..r.nwords {
            if w == Self::FLAG_SLOT {
                continue; // слово знакомства не содержит цифр мантиссы
            }
            let [l, m, h] = r.read_word(ring, w);
            for nib in [h, m, l] {
                if di < 8 {
                    digits[di] = nib % 10;
                    di += 1;
                }
            }
        }
        let f = r.flag_word(ring, Self::FLAG_SLOT);
        // порядок — двухзначное BCD-число: M — десятки, H — единицы (диапазон -99..99)
        let exp_mag = (f.mantissa as i16 % 10) * 10 + (f.exponent as i16 % 10);
        let exponent = if f.sign & 2 != 0 { -exp_mag } else { exp_mag };
        BcdNumber {
            negative: f.sign & 1 != 0,
            exponent: exponent.clamp(-99, 99) as i8,
            digits,
            guard: 0,
            overflow: false,
        }
    }

    pub fn set_x_number(&mut self, n: &BcdNumber) {
        Self::bcd_to_ring(&mut self.ring, &self.x, n);
    }
    pub fn set_y_number(&mut self, n: &BcdNumber) {
        Self::bcd_to_ring(&mut self.ring, &self.y, n);
    }

    fn bcd_to_flag(n: &BcdNumber) -> FlagWord {
        let e = n.exponent as i16;
        let mag = e.abs();
        FlagWord {
            sign: (n.negative as u8 & 1) | (((e < 0) as u8) << 1),
            mantissa: ((mag / 10) % 10) as u8,
            exponent: (mag % 10) as u8,
        }
    }

    fn bcd_to_ring(ring: &mut DelayRing, r: &RingRegister, n: &BcdNumber) {
        let mut di = 0;
        for w in 0..r.nwords {
            if w == Self::FLAG_SLOT {
                r.set_flag_word(ring, w, Self::bcd_to_flag(n));
                continue;
            }
            let mut nibs = [0u8; 3];
            for slot in nibs.iter_mut() {
                if di < 8 {
                    *slot = n.digits[di];
                    di += 1;
                }
            }
            // порядок записи: H->digits[i], M->digits[i+1], L->digits[i+2]
            r.write_word(ring, w, [nibs[2], nibs[1], nibs[0]]);
        }
    }

    /// Окно пользовательского регистра 00..14.
    pub fn user_reg(&self, reg: usize) -> Option<RingRegister> {
        (reg < USER_REGS).then(|| RingRegister::new(base::USER + reg * 9, 3))
    }

    /// Чтение слова `slot` (0..2) пользовательского регистра `reg`.
    pub fn read_user_word(&self, reg: usize, slot: usize) -> Option<[u8; 3]> {
        self.user_reg(reg).map(|r| r.read_word(&self.ring, slot))
    }

    /// Запись слова `slot` (0..2) пользовательского регистра `reg`.
    pub fn write_user_word(&mut self, reg: usize, slot: usize, value: [u8; 3]) -> bool {
        match self.user_reg(reg) {
            Some(r) => {
                r.write_word(&mut self.ring, slot, value);
                true
            }
            None => false,
        }
    }

    /// Чтение слова по непрерывному индексу пользовательской области 0..WORDS_REG.
    pub fn read_user_area_word(&self, word: usize) -> [u8; 3] {
        self.user.read_word(&self.ring, word)
    }

    pub fn write_user_area_word(&mut self, word: usize, value: [u8; 3]) {
        self.user.write_word(&mut self.ring, word, value);
    }

    /// Слово на шине пользовательской памяти; `offset` — смещение от начала области.
    pub fn read_bus_word(&self, offset: usize) -> [u8; 3] {
        self.user
            .read_word(&self.ring, crate::ring::offset_to_word(offset))
    }

    pub fn write_bus_word(&mut self, offset: usize, value: [u8; 3]) {
        self.user
            .write_word(&mut self.ring, crate::ring::offset_to_word(offset), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x_y_and_user_share_the_ring() {
        let mut regs = RegisterFile::new();
        regs.write_x_u12(0, 0x123);
        regs.write_y_u12(1, 0x456);
        assert!(regs.write_user_word(2, 1, [7, 8, 9]));

        assert_eq!(regs.ring.read_triplet(base::X), [3, 2, 1]);
        assert_eq!(regs.ring.read_triplet(base::Y + 3), [6, 5, 4]);
        assert_eq!(regs.ring.read_triplet(base::USER + 2 * 9 + 3), [7, 8, 9]);
    }

    #[test]
    fn flag_word_is_stored_in_ring() {
        let mut regs = RegisterFile::new();
        let n = BcdNumber::parse("-12.5").unwrap_or_default();
        regs.set_x_number(&n);
        assert_eq!(
            regs.x_flag(),
            regs.x.flag_word(&regs.ring, RegisterFile::FLAG_SLOT)
        );
        assert_eq!(
            regs.ring.read_pos(base::X + RegisterFile::FLAG_SLOT * 3),
            regs.x_flag().sign
        );
    }
}
