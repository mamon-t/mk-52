//! ЭНЕРГОНЕЗАВИСИМОЕ ЗАПОМИНАЮЩЕЕ УСТРОЙСТВО (EEPROM 512 байт).
//!
//! По документации МК-52 сохраняет содержимое регистров и программ при выключении
//! питания; микропрограмма выполняет запись побайтно в кольцевом представлении.
//! 45 слов пользовательской области занимают 135 полубайтов, поэтому 512 байт
//! достаточно для полного состояния регистров.

pub const EEPROM_WORDS: usize = 512;
pub const EEPROM_BYTES: usize = EEPROM_WORDS;

#[derive(Clone, Debug)]
pub struct Eeprom {
    pub cells: Vec<u8>, // по одному байту на слово (младшие нибблы значимы)
}

impl Default for Eeprom {
    fn default() -> Self {
        Self::new()
    }
}

impl Eeprom {
    pub fn new() -> Self {
        Self {
            cells: vec![0; EEPROM_WORDS],
        }
    }

    /// Загрузка образа из файла (ровно [`EEPROM_BYTES`] байт). Неверный размер
    /// отвергается, чтобы не принять повреждённый образ за валидный.
    pub fn load_file(path: &str) -> std::io::Result<Self> {
        let data = std::fs::read(path)?;
        if data.len() != EEPROM_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "EEPROM: ожидалось {EEPROM_BYTES} байт, получено {}",
                    data.len()
                ),
            ));
        }
        let mut e = Self::new();
        e.cells.copy_from_slice(&data);
        Ok(e)
    }

    /// Сохранение образа в файл.
    pub fn save_file(&self, path: &str) -> std::io::Result<()> {
        std::fs::write(path, &self.cells)
    }

    /// Имитация записи слова EEPROM: выполняется ВПУ за несколько тактов цикла КС
    /// (в оригинале — последовательная загрузка полубайтов кольца). Здесь фиксируем
    /// только итоговое значение; цикло-точность обеспечивает планировщик Machine.
    pub fn write_word(&mut self, addr: usize, value: u8) {
        self.cells[addr % EEPROM_WORDS] = value & 0xF;
    }

    pub fn read_word(&self, addr: usize) -> u8 {
        self.cells[addr % EEPROM_WORDS] & 0xF
    }

    /// Выгрузка области пользовательских регистров (15 регистров × 3 слова)
    /// в кольцевые окна после включения питания.
    pub fn restore_into(&self, regs: &mut crate::regfile::RegisterFile) {
        for w in 0..crate::regfile::WORDS_REG {
            let base = w * 3;
            let word = [
                self.read_word(base),
                self.read_word(base + 1),
                self.read_word(base + 2),
            ];
            regs.write_user_area_word(w, word);
        }
    }
}
