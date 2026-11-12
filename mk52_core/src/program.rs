//! Загрузка и выгрузка пользовательских программ МК-52 в текстовом формате,
//! совместимом с распространёнными эмуляторами семейства («МК-52 text»):
//!
//! ```text
//! 000   AB0     /АВ
//! 001   AE3     В/П
//! 002   678     КХ
//! ...
//! ```
//!
//! Строка: адрес (3 цифры) + опциональное шестнадцатеричное машинное слово +
//! мнемоника команды. Пустые строки и строки без мнемоники игнорируются.
//! Программа размещается в пользовательской области кольца: 15 регистров по 3
//! 12-битных слова. Одно слово хранит один шаг программы, поэтому текущая
//! модель поддерживает 45 шагов; область целиком помещается в EEPROM 512 байт.

use crate::regfile::{RegisterFile, USER_REGS};
use std::fmt;

/// Ёмкость области пользовательских программ: 15 регистров по 3 слова.
pub const PROGRAM_CAPACITY: usize = USER_REGS * 3;

#[derive(Debug, PartialEq, Eq)]
pub enum ProgramError {
    /// Форматная ошибка с указанием номера строки.
    Format { line: usize, message: String },
    /// Программа не помещается в доступную область памяти.
    TooLarge { needed: usize, capacity: usize },
}

impl fmt::Display for ProgramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProgramError::Format { line, message } => write!(f, "строка {line}: {message}"),
            ProgramError::TooLarge { needed, capacity } => {
                write!(
                    f,
                    "программа слишком велика: {needed} слов (ёмкость {capacity})"
                )
            }
        }
    }
}

impl std::error::Error for ProgramError {}

/// Сопоставление мнемоник МК-52 со значениями поля «тип операции» (биты 4..7 байта).
const MNEMONIC_TO_CODE: &[(&str, u8)] = &[
    ("АВ", 0x2),
    ("В/П", 0xB),
    ("С/П", 0xB), // контекстно; для загрузки различие несущественно (модель ГП упрощена)
    ("КХ", 0x1),
    ("PX", 0x3),
    ("DX", 0x4),
    ("ЕХ", 0x5),
    ("FX", 0x6),
    ("GX", 0x7),
    ("НX", 0x8),
    ("ІХ", 0x9),
    ("JX", 0xA),
    ("CX", 0xB),
    ("+", 0xC),
    ("—", 0xD),
    ("VX", 0xE),
    ("ZX", 0xF),
];

fn mnemonic_to_code(m: &str) -> Option<u8> {
    let up = m.to_uppercase();
    MNEMONIC_TO_CODE
        .iter()
        .find(|(name, _)| *name == up || eq_fold_ascii(name, &up))
        .map(|(_, c)| *c)
}

/// ASCII-нечувствительное к регистру сравнение с учётом русских букв мнемоник.
fn eq_fold_ascii(a: &str, b: &str) -> bool {
    a.chars()
        .map(|c| c.to_ascii_uppercase())
        .eq(b.chars().map(|c| c.to_ascii_uppercase()))
}

/// Кодировать команду в 12-битное слово кольца: H=M=код операции, L=адрес регистра.
fn encode_word(code: u8, reg: u8) -> [u8; 3] {
    [reg & 0xF, code, code]
}

/// Разбор одной строки формата «МК-52 text». Возвращает (адрес, слово) либо None для пропуска.
fn parse_line(line: &str, lineno: usize) -> Result<Option<(usize, [u8; 3])>, ProgramError> {
    let s = line.trim();
    if s.is_empty() || s.starts_with(';') || s.starts_with('#') {
        return Ok(None);
    }
    let mut it = s.split_whitespace();
    let addr_tok = it.next().unwrap_or("");
    let addr: usize = addr_tok.parse().map_err(|_| ProgramError::Format {
        line: lineno,
        message: format!("ожидался адрес из 3 цифр, получено «{addr_tok}»"),
    })?;
    if addr >= 1000 {
        return Err(ProgramError::Format {
            line: lineno,
            message: format!("адрес {addr} > 999"),
        });
    }
    let mut hex_tok: Option<&str> = None;
    let mut mnem_tok: Option<String> = None;
    for tok in it {
        if let Some(rest) = tok.strip_prefix('/') {
            mnem_tok = Some(rest.to_string());
        } else if tok.len() <= 3 && tok.chars().all(|c| c.is_ascii_hexdigit()) {
            hex_tok = Some(tok);
        } else {
            mnem_tok = Some(tok.trim_start_matches('/').to_string());
        }
    }
    let word = if let Some(hex) = hex_tok {
        let v = u16::from_str_radix(hex, 16).map_err(|_| ProgramError::Format {
            line: lineno,
            message: format!("некорректное hex-слово «{hex}»"),
        })?;
        [
            (v & 0xF) as u8,
            ((v >> 4) & 0xF) as u8,
            ((v >> 8) & 0xF) as u8,
        ]
    } else if let Some(m) = mnem_tok {
        let code = mnemonic_to_code(&m).ok_or_else(|| ProgramError::Format {
            line: lineno,
            message: format!("неизвестная мнемоника «{m}»"),
        })?;
        encode_word(code, ((addr / 3) % USER_REGS) as u8)
    } else {
        return Ok(None);
    };
    Ok(Some((addr, word)))
}

/// Загрузить текст программы в кольцевые регистры (область пользовательских программ).
pub fn load_program(text: &str, regs: &mut RegisterFile) -> Result<usize, ProgramError> {
    let mut count = 0usize;
    for (i, line) in text.lines().enumerate() {
        if let Some((addr, word)) = parse_line(line, i + 1)? {
            if addr >= PROGRAM_CAPACITY {
                return Err(ProgramError::TooLarge {
                    needed: addr + 1,
                    capacity: PROGRAM_CAPACITY,
                });
            }
            let reg = addr / 3;
            let slot = addr % 3;
            regs.write_user_word(reg, slot, word);
            count += 1;
        }
    }
    Ok(count)
}

/// Выгрузить область пользовательских программ в текст формата «МК-52 text».
pub fn save_program(regs: &RegisterFile) -> String {
    let mut out = String::new();
    for addr in 0..PROGRAM_CAPACITY {
        let reg = addr / 3;
        let slot = addr % 3;
        let Some([l, m, h]) = regs.read_user_word(reg, slot) else {
            continue;
        };
        // Нулевые слова выводятся явно: иначе при повторной загрузке теряются
        // пустые шаги внутри программы и смещаются адреса.
        let v = ((h as u16) << 8) | ((m as u16) << 4) | l as u16;
        out.push_str(&format!("{addr:03} {v:03X}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_save_load() {
        let mut regs = RegisterFile::new();
        let src = "000 /АВ\n001 /В/П\n002 /КХ\n";
        let n = load_program(src, &mut regs).unwrap();
        assert_eq!(n, 3);
        let dump = save_program(&regs);
        assert_eq!(dump.lines().count(), PROGRAM_CAPACITY);
        // повторная загрузка дампа даёт те же слова
        let mut regs2 = RegisterFile::new();
        let n2 = load_program(&dump, &mut regs2).unwrap();
        assert_eq!(n2, PROGRAM_CAPACITY);
        for addr in 0..PROGRAM_CAPACITY {
            assert_eq!(
                regs.read_user_word(addr / 3, addr % 3),
                regs2.read_user_word(addr / 3, addr % 3),
                "шаг {addr} должен сохраниться, включая нули"
            );
        }
    }

    #[test]
    fn rejects_bad_lines() {
        let mut regs = RegisterFile::new();
        let err = load_program("abc /АВ", &mut regs).unwrap_err();
        assert!(matches!(err, ProgramError::Format { line: 1, .. }));
        let err = load_program("000 /ФУ", &mut regs).unwrap_err();
        assert!(matches!(err, ProgramError::Format { .. }));
    }
}
