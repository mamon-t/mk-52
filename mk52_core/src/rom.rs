//! Микросхема постоянного запоминающего устройства КР1601РР1 (ПЗУ): 1024 слова x 8 бит,
//! хранит микропрограмму (команды МК-52) и таблицы постоянных констант.

pub const ROM_WORDS: usize = 1024;

/// ПЗУ с таблицей микрокоманд. Загружается из файла образа или встроенной заготовки.
#[derive(Clone, Debug)]
pub struct Rom {
    pub data: Vec<u8>,
}

impl Default for Rom {
    fn default() -> Self {
        Self::empty()
    }
}

impl Rom {
    pub fn empty() -> Self {
        Self {
            data: vec![0; ROM_WORDS],
        }
    }

    /// Встроенная тестовая прошивка: короткий цикл NOP/PX, пригодный для
    /// проверки выборки и исполнения без внешнего файла.
    pub fn test_firmware() -> Self {
        let mut rom = Self::empty();
        rom.data[0] = 0x00; // NOP
        rom.data[1] = 0x30; // PX
        rom.data[2] = 0x00; // NOP
        rom
    }

    /// Контрольная сумма образа (XOR всех байтов).
    pub fn checksum(&self) -> u8 {
        self.data.iter().fold(0, |acc, &b| acc ^ b)
    }

    pub fn from_slice(data: &[u8]) -> Self {
        let mut d = vec![0u8; ROM_WORDS];
        let n = data.len().min(ROM_WORDS);
        d[..n].copy_from_slice(&data[..n]);
        Self { data: d }
    }

    /// Чтение микрокоманды по 10-битному адресу.
    pub fn read(&self, addr: usize) -> u8 {
        self.data[addr & 0x3FF]
    }

    /// Загрузка образа из файла. Размер должен быть ровно 1024 байта;
    /// укороченные/дополненные образы отвергаются.
    pub fn load_file(path: &str) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        if bytes.len() != ROM_WORDS {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("ROM: ожидалось {ROM_WORDS} байт, получено {}", bytes.len()),
            ));
        }
        Ok(Self { data: bytes })
    }

    /// Загрузка образа с проверкой ожидаемой контрольной суммы.
    pub fn load_file_checked(path: &str, expected: u8) -> std::io::Result<Self> {
        let rom = Self::load_file(path)?;
        if rom.checksum() != expected {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "ROM: контрольная сумма 0x{:02X}, ожидалось 0x{expected:02X}",
                    rom.checksum()
                ),
            ));
        }
        Ok(rom)
    }
}

/// Коды команд МК-52 (4-битные, старт-код операции + адрес регистра/группы).
/// Соответствуют таблице команд оригинала (К145ИК1302 — ГП).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opcode {
    Nop = 0x0, // / — нет операции
    Kx = 0x1,  // KX — обмен содержимым X и Y
    Ab = 0x2,  // AB — пересылка A->B
    Px = 0x3,  // PX — пересылка P->X (адресная)
    Dx = 0x4,  // DX — пересылка регистра данных в X
    Ex = 0x5,  // EX
    Fx = 0x6,  // FX
    Gx = 0x7,  // GX
    Hx = 0x8,  // HX
    Ix = 0x9,  // IX
    Jx = 0xA,  // JX
    Cx = 0xB,  // CX
    S1 = 0xC,  // +1 (инкремент мантиссы)
    S0 = 0xD,  // -1 (декремент)
    Vx = 0xE,  // VX
    Zx = 0xF,  // ZX
}

impl From<u8> for Opcode {
    fn from(v: u8) -> Self {
        use Opcode::*;
        match v & 0xF {
            0x0 => Nop,
            0x1 => Kx,
            0x2 => Ab,
            0x3 => Px,
            0x4 => Dx,
            0x5 => Ex,
            0x6 => Fx,
            0x7 => Gx,
            0x8 => Hx,
            0x9 => Ix,
            0xA => Jx,
            0xB => Cx,
            0xC => S1,
            0xD => S0,
            0xE => Vx,
            0xF => Zx,
            _ => unreachable!(),
        }
    }
}
