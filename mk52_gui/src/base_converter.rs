//! Помощник перевода систем счисления (панель разработчика).
//!
//! МК-52 оперирует полубайтами (тетрадами), поэтому помощник показывает:
//!  - десятичное значение;
//!  - двоичное, октальное, шестнадцатеричное представления;
//!  - разложение на тетрады — ровно так, как число лежит в кольце задержки.

/// Результат конверсии для отображения в GUI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conversion {
    pub decimal: String,
    pub binary: String,
    pub octal: String,
    pub hex: String,
    /// Тетрады числа (старшая слева) — представление «как в кольце».
    pub nibbles: Vec<u8>,
}

/// Перевести число во все системы счисления + разложение на тетрады.
pub fn convert(value: u64) -> Conversion {
    let mut n = value;
    let mut nibbles = Vec::new();
    loop {
        nibbles.push((n & 0xF) as u8);
        n >>= 4;
        if n == 0 {
            break;
        }
    }
    nibbles.reverse();
    Conversion {
        decimal: format!("{value}"),
        binary: format!("{value:b}"),
        octal: format!("{value:o}"),
        hex: format!("{value:X}"),
        nibbles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_to_all_bases() {
        let c = convert(1234);
        assert_eq!(c.decimal, "1234");
        assert_eq!(c.binary, "10011010010");
        assert_eq!(c.octal, "2322");
        assert_eq!(c.hex, "4D2");
        assert_eq!(c.nibbles, vec![4, 13, 2]);
    }
}
