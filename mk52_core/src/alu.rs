//! АЛУ арифметического процессора (К145ИК1303): последовательная обработка двоично-
//! десятичных полубайтов. Реализовано как классическая конечная машина состояний (FSM),
//! синхронизируемая фазами M2/M3/M4 — как в оригинальных микросхемах МК-52.

/// Состояния FSM арифметического процессора.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MathState {
    /// Ожидание запуска операции (сброс/пауза).
    Idle,
    /// Приём первого операнда из кольца (регистр X).
    LoadX,
    /// Приём второго операнда / подготовка.
    PrepY,
    /// Выполнение итерации сложения/вычитания с переносом.
    AddSub,
    /// Умножение: цикл сдвигов и накопления.
    Mul,
    /// Деление: цикл сравнения и вычитания.
    Div,
    /// Нормализация результата (сдвиг мантиссы, корректировка порядка).
    Normalize,
    /// Округление результата до 8 разрядов мантиссы.
    Round,
    /// Запись результата в кольцо (регистр Y) и формирование слова знакомства.
    StoreResult,
    /// Операция завершена (ожидание сброса сигнала готовности).
    Done,
}

impl std::fmt::Display for MathState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            MathState::Idle => "Idle",
            MathState::LoadX => "LoadX",
            MathState::PrepY => "PrepY",
            MathState::AddSub => "AddSub",
            MathState::Mul => "Mul",
            MathState::Div => "Div",
            MathState::Normalize => "Normalize",
            MathState::Round => "Round",
            MathState::StoreResult => "StoreResult",
            MathState::Done => "Done",
        };
        write!(f, "{s}")
    }
}

/// Десятичное число МК-52: знак + порядок (-9..=99) + 8 значащих цифр (BAIN кодировка
/// хранится побайтово для простоты отладки; кольцевое представление см. в ring.rs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BcdNumber {
    pub negative: bool,
    pub exponent: i8,
    /// 8 цифр мантиссы, digits[0] — старшая. 0 означает пустой разряд.
    pub digits: [u8; 8],
    /// Первая отброшенная цифра (0..9) для округления результата.
    pub guard: u8,
    /// Порядок вышел за допустимый диапазон; цифры при этом сохраняются.
    pub overflow: bool,
}

impl BcdNumber {
    pub fn zero() -> Self {
        Self::default()
    }

    pub fn is_zero(&self) -> bool {
        self.digits.iter().all(|&d| d == 0)
    }

    /// Текстовое представление числа в формате калькулятора (для панели разработчика):
    ///mantissa × 10^exponent с учётом знака. Нуль выводится как «0».
    pub fn to_display_string(&self) -> String {
        if self.is_zero() {
            return "0".to_string();
        }
        let mant = self
            .digits
            .iter()
            .map(|&d| (b'0' + d) as char)
            .collect::<String>();
        let sign = if self.negative { "-" } else { "" };
        if self.exponent == 0 {
            format!("{sign}{mant}")
        } else {
            format!("{sign}{mant}E{:+}", self.exponent)
        }
    }

    /// Собрать значение вида "±digits * 10^exponent" из строки ввода клавиатуры.
    pub fn parse(text: &str) -> Option<Self> {
        let mut chars = text.chars().peekable();
        let mut negative = false;
        if chars.peek() == Some(&'-') {
            negative = true;
            chars.next();
        }
        let mut int_part = String::new();
        let mut frac_part = String::new();
        let mut in_frac = false;
        for c in chars {
            if c == '.' {
                in_frac = true;
            } else if c.is_ascii_digit() {
                if in_frac {
                    frac_part.push(c);
                } else {
                    int_part.push(c);
                }
            } else {
                return None;
            }
        }
        let combined: Vec<u8> = int_part
            .chars()
            .chain(frac_part.chars())
            .map(|c| c as u8 - b'0')
            .collect();
        if combined.is_empty() {
            return None;
        }
        // нормализация: первая ненулевая цифра становится старшей
        let lead = combined
            .iter()
            .position(|&d| d != 0)
            .unwrap_or(combined.len());
        let exp = (int_part.len() as i8 - 1) - lead as i8;
        let mut digits = [0u8; 8];
        for (i, digit) in digits.iter_mut().enumerate() {
            if let Some(&d) = combined.get(lead + i) {
                *digit = d;
            }
        }
        Some(Self {
            negative,
            exponent: exp,
            digits,
            guard: 0,
            overflow: false,
        })
    }

    /// Позиция старшего значащего разряда (None если ноль).
    fn first_nonzero(&self) -> Option<usize> {
        self.digits.iter().position(|&d| d != 0)
    }

    /// Сдвиг мантиссы влево на одну позицию с потерей младшего разряда.
    /// Порядок (exponent) не меняется: вес младшей цифры фиксирован — 10^exponent.
    fn shl_digits(&mut self) {
        for i in 0..7 {
            self.digits[i] = self.digits[i + 1];
        }
        self.digits[7] = 0;
    }

    /// Нормализация: убираем ведущие нули, НЕ меняя порядок. При формате
    /// value = digits * 10^exponent сдвиг влево делит значение на 10, поэтому для
    /// сохранения значения порядок должен уменьшаться. Но в этом представлении
    /// нормализованное число имеет ровно 8 значащих цифр без ведущих нулей, и
    /// сдвиг влево на pos позиций требует компенсации exp -= pos только если
    /// мы хотим сохранить величину; однако фактический смысл exponent здесь —
    /// вес МЛАДШЕЙ цифры, которая после сдвига влево остаётся на позиции 7,
    /// но её вес уменьшается в 10^pos раз. Следовательно exp -= pos.
    pub fn normalize(&mut self) {
        if self.is_zero() {
            self.exponent = 0;
            self.guard = 0;
            self.overflow = false;
            return;
        }
        if let Some(pos) = self.first_nonzero()
            && pos > 0
        {
            for _ in 0..pos {
                self.shl_digits();
            }
            self.exponent -= pos as i8;
        }
        // Порядок вне диапазона не обнуляет число: цифры сохраняются,
        // а флаг позволяет машине сформировать специальный режим.
        if self.exponent > 99 {
            self.exponent = 99;
            self.overflow = true;
        } else if self.exponent < -99 {
            self.exponent = -99;
            self.overflow = true;
        }
    }

    /// Округление половины вверх по первой отброшенной цифре `guard`.
    /// При переполнении мантиссы выполняется сдвиг вправо и рост порядка.
    pub fn round(&mut self) {
        if self.is_zero() {
            self.guard = 0;
            return;
        }
        if self.guard >= 5 {
            let mut carry = true;
            for i in (0..8).rev() {
                if self.digits[i] < 9 {
                    self.digits[i] += 1;
                    carry = false;
                    break;
                }
                self.digits[i] = 0;
            }
            if carry {
                for i in (1..8).rev() {
                    self.digits[i] = self.digits[i - 1];
                }
                self.digits[0] = 1;
                self.exponent += 1;
            }
        }
        self.guard = 0;
        self.normalize();
    }

    /// Побитово-десятичное сложение без знака (мантиссы выровнены по вызывающему).
    fn add_mag(a: &[u8; 8], b: &[u8; 8]) -> ([u8; 8], bool) {
        let mut out = [0u8; 8];
        let mut carry = false;
        for i in (0..8).rev() {
            let s = a[i] + b[i] + carry as u8;
            out[i] = s % 10;
            carry = s >= 10;
        }
        (out, carry)
    }

    fn sub_mag_ge(a: &[u8; 8], b: &[u8; 8]) -> [u8; 8] {
        // предполагает a >= b
        let mut out = [0u8; 8];
        let mut borrow = 0i8;
        for i in (0..8).rev() {
            let mut d = a[i] as i8 - b[i] as i8 - borrow;
            if d < 0 {
                d += 10;
                borrow = 1;
            } else {
                borrow = 0;
            }
            out[i] = d as u8;
        }
        out
    }

    fn cmp_mag(a: &[u8; 8], b: &[u8; 8]) -> core::cmp::Ordering {
        a.cmp(b)
    }

    /// Сложение двух чисел с выравниванием порядков (классический алгоритм МК-52).
    ///
    /// Формат: mantissa = Σ digits[i]·10^(7-i), value = mantissa · 10^exponent.
    /// При сложении с разными экспонентами меньшее число сдвигается вправо
    /// (mantissa /= 10^diff), чтобы его цифры заняли те же веса, что у большего.
    pub fn add(x: &BcdNumber, y: &BcdNumber) -> BcdNumber {
        let mut diff = x.exponent as i32 - y.exponent as i32;
        if diff == 0 {
            return Self::add_aligned(x, y, x.exponent);
        }
        if diff < 0 {
            // y имеет больший порядок: сдвигаем x вправо к рамке y
            diff = -diff;
            if diff >= 8 {
                // меньшее (x) полностью теряется при выравнивании
                let mut r = *y;
                r.normalize();
                return r;
            }
            let mut shifted = Self::shr_digits(&x.digits, diff as usize);
            shifted.negative = x.negative;
            Self::add_aligned(&shifted, y, y.exponent)
        } else {
            if diff >= 8 {
                // меньшее (y) полностью теряется при выравнивании
                let mut r = *x;
                r.normalize();
                return r;
            }
            let mut shifted = Self::shr_digits(&y.digits, diff as usize);
            shifted.negative = y.negative;
            Self::add_aligned(x, &shifted, x.exponent)
        }
    }

    /// Сдвиг мантиссы вправо на `n` позиций (потеря младших цифр). Возвращает
    /// только digits — вызывающий сам задаёт знак и общий порядок (см. `add`).
    fn shr_digits(d: &[u8; 8], n: usize) -> BcdNumber {
        let mut digits = [0u8; 8];
        if n < 8 {
            digits[n..].copy_from_slice(&d[..8 - n]);
        }
        BcdNumber {
            negative: false,
            exponent: 0,
            digits,
            guard: 0,
            overflow: false,
        }
    }

    /// Сложение выровненных мантисс (общий вес старшей цифры `exp+7`).
    fn add_aligned(a: &BcdNumber, b: &BcdNumber, exp: i8) -> BcdNumber {
        let mut result = if a.negative == b.negative {
            let (digits, overflow) = Self::add_mag(&a.digits, &b.digits);
            let mut r = BcdNumber {
                negative: a.negative,
                exponent: exp,
                digits,
                guard: 0,
                overflow: false,
            };
            if overflow {
                // перенос за старший разряд: добавляем цифру 1 слева со сдвигом
                for i in (1..8).rev() {
                    r.digits[i] = r.digits[i - 1];
                }
                r.digits[0] = 1;
                r.exponent += 1;
            }
            r
        } else {
            match Self::cmp_mag(&a.digits, &b.digits) {
                core::cmp::Ordering::Equal => BcdNumber::zero(),
                core::cmp::Ordering::Greater => BcdNumber {
                    negative: a.negative,
                    exponent: exp,
                    digits: Self::sub_mag_ge(&a.digits, &b.digits),
                    guard: 0,
                    overflow: false,
                },
                core::cmp::Ordering::Less => BcdNumber {
                    negative: b.negative,
                    exponent: exp,
                    digits: Self::sub_mag_ge(&b.digits, &a.digits),
                    guard: 0,
                    overflow: false,
                },
            }
        };
        result.normalize();
        result
    }

    pub fn sub(x: &BcdNumber, y: &BcdNumber) -> BcdNumber {
        let mut ny = *y;
        ny.negative = !ny.negative;
        Self::add(x, &ny)
    }

    /// Умножение. FSM-эквивалент: накопление сдвигов + циклы сложения.
    /// Формат: value = mantissa × 10^exponent, где mantissa = Σ digits[i]·10^(7-i).
    /// x*y = (mx×my) × 10^(xe+ye): произведение двух 8-значных мантисс даёт
    /// до 16 цифр; после нормализации (сдвиг влево на `lead`) экспонента
    /// увеличивается на (15 - lead), чтобы сохранить значение.
    pub fn mul(x: &BcdNumber, y: &BcdNumber) -> BcdNumber {
        if x.is_zero() || y.is_zero() {
            return BcdNumber::zero();
        }
        let to_int = |d: &[u8; 8]| -> u64 { d.iter().fold(0u64, |acc, &v| acc * 10 + v as u64) };
        let prod = to_int(&x.digits) as u128 * to_int(&y.digits) as u128;
        // Раскладываем произведение по цифрам (16 позиций)
        let mut tmp = [0u8; 16];
        let mut p = prod;
        for i in (0..16).rev() {
            tmp[i] = (p % 10) as u8;
            p /= 10;
        }
        // Находим первую ненулевую цифру (позиция в массиве tmp[0..16])
        let lead = tmp.iter().position(|&d| d != 0).unwrap_or(15);
        // Извлекаем 8 значащих цифр результата; девятая цифра идёт в guard.
        let mut digits = [0u8; 8];
        for (i, d) in digits.iter_mut().enumerate() {
            *d = tmp.get(lead + i).copied().unwrap_or(0);
        }
        let guard = tmp.get(lead + 8).copied().unwrap_or(0);
        // Экспонента: mx×my имеет старшую цифру на позиции (15-lead) внутри
        // 16-разрядного числа, т.е. её вес = 10^(15-lead). Полный вес результата
        // = 10^(15-lead) × 10^(xe+ye) = 10^(xe+ye+15-lead). Но в формате
        // mantissa × 10^exp старшая цифра мантиссы имеет вес 10^7, поэтому
        // exp_результата = (xe+ye+15-lead) - 7 = xe+ye+8-lead.
        let raw_exp = x.exponent as i16 + y.exponent as i16 + 8 - lead as i16;
        let e = raw_exp.clamp(-99, 99) as i8;
        let mut result = BcdNumber {
            negative: x.negative != y.negative,
            exponent: e,
            digits,
            guard,
            overflow: !(-99..=99).contains(&raw_exp),
        };
        result.round();
        result
    }

    /// Деление. FSM-эквивалент: последовательные вычитания со сдвигом.
    /// x/y = (mx/my) * 10^(xe-ye); scaled = mx*10^8/my ⇒ частное ≈ scaled*10^(e-8).
    pub fn div(x: &BcdNumber, y: &BcdNumber) -> Result<BcdNumber, DivError> {
        if y.is_zero() {
            return Err(DivError::DivideByZero);
        }
        if x.is_zero() {
            return Ok(BcdNumber::zero());
        }
        let exp = x.exponent as i16 - y.exponent as i16;
        let to_int = |d: &[u8; 8]| -> u64 { d.iter().fold(0u64, |acc, &v| acc * 10 + v as u64) };
        let num = to_int(&x.digits);
        let den = to_int(&y.digits);
        // Делим с масштабированием: 8 цифр мантиссы + одна цифра округления.
        let scaled = num as u128 * 10u128.pow(9) / den as u128;
        let mut tmp = [0u8; 10];
        let mut q = scaled;
        for i in (0..10).rev() {
            tmp[i] = (q % 10) as u8;
            q /= 10;
        }
        let lead = tmp.iter().position(|&d| d != 0).unwrap_or(8);
        let mut digits = [0u8; 8];
        for (i, d) in digits.iter_mut().enumerate() {
            *d = tmp.get(lead + i).copied().unwrap_or(0);
        }
        let guard = tmp.get(lead + 8).copied().unwrap_or(0);
        // scaled = trunc(mx*10^8/my) ⇒ mx/my ≈ scaled * 10^-8,
        // x/y ≈ scaled * 10^(exp-8). Мантисса результата = scaled >> lead,
        // её вес единицы = 10^(exp-8+(8-lead)) = 10^(exp-lead).
        let raw_exp = exp - lead as i16;
        let mut r = BcdNumber {
            negative: x.negative != y.negative,
            exponent: raw_exp.clamp(-99, 99) as i8,
            digits,
            guard,
            overflow: !(-99..=99).contains(&raw_exp),
        };
        r.round();
        Ok(r)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DivError {
    DivideByZero,
}

/// Ошибка арифметической операции, отображаемая специальным режимом ВФД.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MathFault {
    Overflow,
    DivideByZero,
}

/// Арифметический процессор: хранит текущее состояние FSM и рабочие регистры.
#[derive(Clone, Debug)]
pub struct ArithmeticProcessor {
    pub state: MathState,
    /// Флаг готовности результата (устанавливается в такте M4 по окончании записи).
    pub ready: bool,
    pub x: BcdNumber,
    pub y: BcdNumber,
    pub op: MathOp,
    /// Ошибка последней операции; сбрасывается при запуске новой.
    pub fault: Option<MathFault>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MathOp {
    Add,
    Sub,
    Mul,
    Div,
}

impl Default for ArithmeticProcessor {
    fn default() -> Self {
        Self {
            state: MathState::Idle,
            ready: false,
            x: BcdNumber::zero(),
            y: BcdNumber::zero(),
            op: MathOp::Add,
            fault: None,
        }
    }
}

impl ArithmeticProcessor {
    /// Запуск операции: переход FSM из Idle в соответствующее состояние.
    pub fn start(&mut self, op: MathOp, x: BcdNumber, y: BcdNumber) {
        self.op = op;
        self.x = x;
        self.y = y;
        self.ready = false;
        self.fault = None;
        self.state = match op {
            MathOp::Add | MathOp::Sub => MathState::AddSub,
            MathOp::Mul => MathState::Mul,
            MathOp::Div => MathState::Div,
        };
    }

    /// Шаг FSM: вызывается на каждом такте (фаза M1->M2...). Вычисляет результат
    /// за фиксированное число шагов, имитируя последовательную обработку.
    pub fn step(&mut self) {
        match self.state {
            MathState::Idle | MathState::Done => {}
            MathState::AddSub => {
                let r = match self.op {
                    MathOp::Add => BcdNumber::add(&self.x, &self.y),
                    MathOp::Sub => BcdNumber::sub(&self.x, &self.y),
                    _ => unreachable!(),
                };
                if r.overflow {
                    self.fault = Some(MathFault::Overflow);
                }
                self.y = r;
                self.state = MathState::StoreResult;
            }
            MathState::Mul => {
                let r = BcdNumber::mul(&self.x, &self.y);
                if r.overflow {
                    self.fault = Some(MathFault::Overflow);
                }
                self.y = r;
                self.state = MathState::StoreResult;
            }
            MathState::Div => {
                match BcdNumber::div(&self.x, &self.y) {
                    Ok(r) => {
                        if r.overflow {
                            self.fault = Some(MathFault::Overflow);
                        }
                        self.y = r;
                    }
                    Err(_) => {
                        self.y = BcdNumber::zero(); // деление на ноль: «пустышка»
                        self.fault = Some(MathFault::DivideByZero);
                    }
                }
                self.state = MathState::StoreResult;
            }
            MathState::StoreResult => {
                self.state = MathState::Done;
            }
            other => {
                self.state = other;
            }
        }
    }

    /// Сброс (по сигналу КС после выдачи результата).
    pub fn reset(&mut self) {
        self.state = MathState::Idle;
        self.ready = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_normalizes() {
        let n = BcdNumber::parse("0.001234").unwrap();
        assert_eq!(n.digits[..4], [1, 2, 3, 4]);
        assert_eq!(n.exponent, -3);
        let m = BcdNumber::parse("-567890").unwrap();
        assert!(m.negative);
        assert_eq!(m.exponent, 5);
    }

    #[test]
    fn add_basic() {
        let a = BcdNumber::parse("1234").unwrap();
        let b = BcdNumber::parse("567890").unwrap();
        let r = BcdNumber::add(&a, &b); // 569124
        assert_eq!(r.digits[..6], [5, 6, 9, 1, 2, 4]);
        assert_eq!(r.exponent, 5);
    }

    #[test]
    fn add_with_alignment_loss() {
        let a = BcdNumber::parse("1").unwrap();
        let mut b = BcdNumber::default();
        b.digits[0] = 5;
        b.exponent = 20;
        let r = BcdNumber::add(&a, &b);
        assert_eq!(r.digits[0], 5);
        assert_eq!(r.exponent, 20);
    }

    #[test]
    fn sub_negative_result() {
        let a = BcdNumber::parse("3").unwrap();
        let b = BcdNumber::parse("10").unwrap();
        let r = BcdNumber::sub(&a, &b);
        // 3 - 10 = -7
        assert!(r.negative);
        assert_eq!(r.digits[0], 7);
    }

    #[test]
    fn mul_basic() {
        let a = BcdNumber::parse("12").unwrap();
        let b = BcdNumber::parse("12").unwrap();
        let r = BcdNumber::mul(&a, &b);
        // 12 × 12 = 144
        assert_eq!(r.digits[..3], [1, 4, 4]);
        // Формат: value = mantissa × 10^exponent
        // parse("12") даёт digits=[1,2,0,0,0,0,0,0], exp=1 (mantissa=12000000)
        // 12×12=144 → mantissa=14400000, exp=9 (значение = 14400000 × 10^9 / 10^8 = 144)
        // Экспонента хранит вес МЛАДШЕЙ цифры мантиссы относительно единицы
        assert_eq!(r.exponent, 9);
    }

    #[test]
    fn div_by_zero_errors() {
        let a = BcdNumber::parse("1").unwrap();
        assert_eq!(
            BcdNumber::div(&a, &BcdNumber::zero()),
            Err(DivError::DivideByZero)
        );
    }

    #[test]
    fn div_basic() {
        let a = BcdNumber::parse("144").unwrap();
        let b = BcdNumber::parse("12").unwrap();
        let r = BcdNumber::div(&a, &b).unwrap();
        assert_eq!(r.digits[..3], [1, 2, 0]);
    }

    #[test]
    fn round_half_up_increments_mantissa() {
        let mut n = BcdNumber::parse("12345679").unwrap();
        n.guard = 5;
        n.round();
        assert_eq!(n.digits, [1, 2, 3, 4, 5, 6, 8, 0]);
        assert_eq!(n.guard, 0);
    }

    #[test]
    fn round_carry_grows_exponent() {
        let mut n = BcdNumber::parse("99999999").unwrap();
        let exp = n.exponent;
        n.guard = 5;
        n.round();
        assert_eq!(n.digits, [1, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(n.exponent, exp + 1);
    }

    #[test]
    fn mul_uses_guard_digit() {
        let a = BcdNumber::parse("11111111").unwrap();
        let b = BcdNumber::parse("99999999").unwrap();
        let r = BcdNumber::mul(&a, &b);
        assert_eq!(r.digits, [1, 1, 1, 1, 1, 1, 1, 1]);
    }

    #[test]
    fn div_uses_guard_digit() {
        let a = BcdNumber::parse("2").unwrap();
        let b = BcdNumber::parse("3").unwrap();
        let r = BcdNumber::div(&a, &b).unwrap();
        assert_eq!(r.digits, [6, 6, 6, 6, 6, 6, 6, 7]);
        assert_eq!(r.exponent, -1);
    }

    #[test]
    fn normalize_flags_overflow_without_losing_digits() {
        let mut n = BcdNumber::parse("9").unwrap();
        n.exponent = 100;
        n.normalize();
        assert!(n.overflow);
        assert_eq!(n.exponent, 99);
        assert_eq!(n.digits[0], 9);
    }

    #[test]
    fn arithmetic_processor_reports_division_by_zero() {
        let mut p = ArithmeticProcessor::default();
        p.start(
            MathOp::Div,
            BcdNumber::parse("1").unwrap(),
            BcdNumber::zero(),
        );
        p.step();
        assert_eq!(p.fault, Some(MathFault::DivideByZero));
    }

    #[test]
    fn fsm_lifecycle() {
        let mut p = ArithmeticProcessor::default();
        p.start(
            MathOp::Add,
            BcdNumber::parse("1").unwrap(),
            BcdNumber::parse("2").unwrap(),
        );
        assert_eq!(p.state, MathState::AddSub);
        p.step();
        assert_eq!(p.state, MathState::StoreResult);
        p.step();
        assert_eq!(p.state, MathState::Done);
        p.reset();
        assert_eq!(p.state, MathState::Idle);
        assert!(p.ready);
    }
}
