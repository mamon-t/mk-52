//! Устройства ввода-вывода МК-52: вакуумный флуоресцентный индикатор (ВFD, К745ГФ3)
//! и клавиатура, обслуживаемые системным контроллером в цикле сканирования.

/// Один сегмент/разряд индикации. Индикатор имеет 10 разрядов; каждый разряд
/// кодируется полубайтом кода сегментов + признаком десятичной точки.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DigitCell {
    /// Код разряда по таблице К745ГФ3 (биты сегментов a..g, dp).
    pub code: u8,
    /// Активен ли разряд в текущем цикле мультиплексирования.
    pub enabled: bool,
}

/// Модель ВFD: регистр данных КС загружается на такте M3 в момент обслуживания
/// активного разряда; яркость/«хвосты» после выключения не моделируются (цикло-точная
/// логика приоритетна, визуальные сглаживающие эффекты — задача GUI-этапа).
#[derive(Clone, Debug, Default)]
pub struct VfdDisplay {
    pub digits: [DigitCell; 10],
}

impl VfdDisplay {
    pub fn new() -> Self {
        Self::default()
    }

    /// Обслуживание разряда: вызывается КС на соответствующем такте цикла.
    pub fn service_digit(&mut self, digit: usize, code: u8) {
        if digit < 10 {
            self.digits[digit] = DigitCell {
                code,
                enabled: true,
            };
        }
    }

    /// Декодирование кода разряда в символ (таблица сегментов К745ГФ3 для цифр и букв).
    pub fn decode(code: u8) -> char {
        match code {
            0x0 => ' ',
            0x1 => '-',
            0x2 => '1',
            0x3 => '2',
            0x4 => '3',
            0x5 => '4',
            0x6 => '5',
            0x7 => '6',
            0x8 => '7',
            0x9 => '8',
            0xA => '9',
            0xB => 'A',
            0xC => 'Б',
            0xD => 'C',
            0xE => 'Е',
            0xF => 'Х',
            _ => '?',
        }
    }

    /// Символьное представление текущего состояния (для отладки headless).
    pub fn render(&self) -> String {
        self.digits.iter().map(|d| Self::decode(d.code)).collect()
    }
}

/// Клавиатура МК-52: матрица 10x? (строки опрашиваются дешифратором КС в окне
/// сканирования). Хранит «нажатые» линии как внешний сигнал от хоста/GUI.
#[derive(Clone, Debug, Default)]
pub struct Keyboard {
    /// Битовая маска замкнутых ключей (линия строки x столбца), индекс = row*cols+col.
    pressed: Vec<bool>,
    rows: usize,
    cols: usize,
}

pub const KB_ROWS: usize = 10;
pub const KB_COLS: usize = 5;

impl Keyboard {
    pub fn new() -> Self {
        Self {
            pressed: vec![false; KB_ROWS * KB_COLS],
            rows: KB_ROWS,
            cols: KB_COLS,
        }
    }

    pub fn press(&mut self, row: usize, col: usize) {
        if row < self.rows && col < self.cols {
            self.pressed[row * self.cols + col] = true;
        }
    }

    pub fn release(&mut self, row: usize, col: usize) {
        if row < self.rows && col < self.cols {
            self.pressed[row * self.cols + col] = false;
        }
    }

    pub fn is_pressed(&self, row: usize, col: usize) -> bool {
        row < self.rows && col < self.cols && self.pressed[row * self.cols + col]
    }

    /// Опрос строки дешифратором КС: возвращает маску замкнутых столбцов данной строки.
    pub fn scan_row(&self, row: usize) -> u8 {
        if row >= self.rows {
            return 0;
        }
        let mut mask = 0u8;
        for c in 0..self.cols {
            if self.is_pressed(row, c) {
                mask |= 1 << c;
            }
        }
        mask
    }
}
