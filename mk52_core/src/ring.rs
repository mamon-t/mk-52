//! Кольцевая шина памяти МК-52 (реализация «кольца задержки» микросхем К145ИК1302/1303).
//!
//! Согласно документации:
//!  - кольцо содержит 2352 позиции, сдвиг на одну позицию происходит на каждом тактовом
//!    импульсе F1 (4 фазы M1..M4 в цикле -> один сдвиг за цикл);
//!  - регистры процессоров читаются/записываются «каждым третьим битом»:
//!    R1 использует позиции 3k+0, R2 — 3k+1, R3 — 3k+2;
//!  - данные хранятся полубайтами (4 бита) в каждой позиции;
//!  - адрес позиции вычисляется как `(head + offset) mod 2352`.
//!
//! `DelayRing` — единственное хранилище данных. `RingRegister` — лишь описание
//! окна в кольце: базовой позиции и количества слов. Благодаря этому X, Y и
//! пользовательская память не дублируются в отдельных массивах.

pub const RING_LEN: usize = 2352;

/// Кольцевой регистр — окно из `nwords` слов (по 3 полубайта) в общем кольце.
///
/// Данные не хранятся в самом окне: чтение и запись выполняются через
/// [`DelayRing`], поэтому любые изменения сразу видны и контроллеру памяти,
/// и панели разработчика.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RingRegister {
    /// Абсолютная позиция первого полубайта окна в кольце.
    pub base: usize,
    /// Количество 12-битных слов в окне.
    pub nwords: usize,
}

impl RingRegister {
    pub const fn new(base: usize, nwords: usize) -> Self {
        Self { base, nwords }
    }

    /// Позиция первого полубайта слова `word` с автоматическим зацикливанием
    /// переполнения адреса (как PK в оригинале).
    pub fn word_pos(&self, word: usize) -> usize {
        let w = word % self.nwords;
        (self.base + w * 3) % RING_LEN
    }

    pub fn read_word(&self, ring: &DelayRing, word: usize) -> [u8; 3] {
        ring.read_triplet(self.word_pos(word))
    }

    pub fn write_word(&self, ring: &mut DelayRing, word: usize, value: [u8; 3]) {
        ring.write_triplet(self.word_pos(word), value);
    }

    pub fn read_h(&self, ring: &DelayRing, word: usize) -> u8 {
        ring.read_pos(self.word_pos(word) + 2) & 0x0F
    }

    pub fn read_m(&self, ring: &DelayRing, word: usize) -> u8 {
        ring.read_pos(self.word_pos(word) + 1) & 0x0F
    }

    pub fn read_l(&self, ring: &DelayRing, word: usize) -> u8 {
        ring.read_pos(self.word_pos(word)) & 0x0F
    }

    pub fn write_h(&self, ring: &mut DelayRing, word: usize, v: u8) {
        ring.write_pos(self.word_pos(word) + 2, v);
    }

    pub fn write_m(&self, ring: &mut DelayRing, word: usize, v: u8) {
        ring.write_pos(self.word_pos(word) + 1, v);
    }

    pub fn write_l(&self, ring: &mut DelayRing, word: usize, v: u8) {
        ring.write_pos(self.word_pos(word), v);
    }

    /// Чтение 12-битного слова как u16 (H:M:L).
    pub fn read_u12(&self, ring: &DelayRing, word: usize) -> u16 {
        let [l, m, h] = self.read_word(ring, word);
        ((h as u16) << 8) | ((m as u16) << 4) | l as u16
    }

    /// Запись 12-битного слова.
    pub fn write_u12(&self, ring: &mut DelayRing, word: usize, v: u16) {
        self.write_word(
            ring,
            word,
            [
                (v & 0xF) as u8,
                ((v >> 4) & 0xF) as u8,
                ((v >> 8) & 0xF) as u8,
            ],
        );
    }

    /// Слово знакомства (flag word): L=знак(1=отрицательный), H=экспонента, M=мантисса.
    pub fn flag_word(&self, ring: &DelayRing, word: usize) -> FlagWord {
        FlagWord {
            sign: self.read_l(ring, word) & 1,
            exponent: self.read_h(ring, word),
            mantissa: self.read_m(ring, word),
        }
    }

    pub fn set_flag_word(&self, ring: &mut DelayRing, word: usize, f: FlagWord) {
        self.write_l(ring, word, f.sign);
        self.write_h(ring, word, f.exponent);
        self.write_m(ring, word, f.mantissa);
    }
}

/// Слово знакомства: признак знака числа и номер регистра, в котором находится число.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlagWord {
    pub sign: u8,     // бит знака (L-полубайт, младший бит)
    pub exponent: u8, // экспонента / код регистра (H)
    pub mantissa: u8, // мантисса / additional field (M)
}

/// Кольцо задержки: буфер из `RING_LEN` полубайтов плюс указатель головы.
///
/// Позиции кольца адресуются абсолютно; `head` отражает текущую фазу
/// циркуляции и используется для наблюдаемости и расчёта задержек.
#[derive(Clone, Debug)]
pub struct DelayRing {
    buf: [u8; RING_LEN],
    head: usize,
}

impl Default for DelayRing {
    fn default() -> Self {
        Self::new()
    }
}

impl DelayRing {
    pub fn new() -> Self {
        Self {
            buf: [0; RING_LEN],
            head: 0,
        }
    }

    /// Сдвиг кольца на одну позицию (на каждом F1).
    pub fn shift(&mut self) {
        self.head = (self.head + 1) % RING_LEN;
    }

    pub fn head(&self) -> usize {
        self.head
    }

    /// Абсолютная позиция кольца для смещения `offset` от головы.
    pub fn position(&self, offset: usize) -> usize {
        (self.head + offset) % RING_LEN
    }

    pub fn read_at_offset(&self, offset: usize) -> u8 {
        self.buf[self.position(offset)] & 0x0F
    }

    pub fn write_at_offset(&mut self, offset: usize, value: u8) {
        self.buf[self.position(offset)] = value & 0x0F;
    }

    /// Прямое чтение/запись позиции (для контроллера памяти).
    pub fn read_pos(&self, pos: usize) -> u8 {
        self.buf[pos % RING_LEN] & 0x0F
    }

    pub fn write_pos(&mut self, pos: usize, value: u8) {
        self.buf[pos % RING_LEN] = value & 0x0F;
    }

    /// Чтение трёх полубайтов слова, начиная с абсолютной позиции `pos`.
    pub fn read_triplet(&self, pos: usize) -> [u8; 3] {
        [
            self.read_pos(pos),
            self.read_pos(pos + 1),
            self.read_pos(pos + 2),
        ]
    }

    /// Запись трёх полубайтов слова, начиная с абсолютной позиции `pos`.
    pub fn write_triplet(&mut self, pos: usize, value: [u8; 3]) {
        self.write_pos(pos, value[0]);
        self.write_pos(pos + 1, value[1]);
        self.write_pos(pos + 2, value[2]);
    }

    /// Найти смещение от головы для абсолютной позиции (используется при арбитраже шины).
    pub fn offset_of(&self, pos: usize) -> usize {
        (pos + RING_LEN - self.head) % RING_LEN
    }
}

/// Преобразование адреса слова МК-52 (0..=51) в смещение в кольце.
/// Каждое слово занимает 3 последовательные позиции.
pub fn word_to_offset(word: usize) -> usize {
    word * 3
}

/// Обратное преобразование: смещение -> адрес слова.
pub fn offset_to_word(offset: usize) -> usize {
    offset / 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_wraps_head() {
        let mut r = DelayRing::new();
        for _ in 0..(RING_LEN + 5) {
            r.shift();
        }
        assert_eq!(r.head(), 5);
    }

    #[test]
    fn register_word_roundtrip_and_wraparound() {
        let mut ring = DelayRing::new();
        let reg = RingRegister::new(0, 3);
        reg.write_u12(&mut ring, 0, 0xABC);
        assert_eq!(reg.read_u12(&ring, 0), 0xABC);
        // переполнение адреса зацикливается (как PK в оригинале)
        reg.write_u12(&mut ring, 3, 0x111);
        assert_eq!(reg.read_u12(&ring, 0), 0x111);
    }

    #[test]
    fn register_data_lives_in_the_shared_ring() {
        let mut ring = DelayRing::new();
        let reg = RingRegister::new(9, 3);
        reg.write_word(&mut ring, 0, [1, 2, 3]);
        assert_eq!(ring.read_triplet(9), [1, 2, 3]);
        assert_eq!(ring.read_triplet(0), [0, 0, 0]);
    }

    #[test]
    fn flag_word_roundtrip() {
        let mut ring = DelayRing::new();
        let reg = RingRegister::new(0, 1);
        let f = FlagWord {
            sign: 1,
            exponent: 7,
            mantissa: 3,
        };
        reg.set_flag_word(&mut ring, 0, f);
        assert_eq!(reg.flag_word(&ring, 0), f);
    }
}
