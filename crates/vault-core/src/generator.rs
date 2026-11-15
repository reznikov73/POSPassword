use rand::{RngExt, rng};

use crate::{Error, Result};

const LOWER: &str = "abcdefghijkmnopqrstuvwxyz";
const UPPER: &str = "ABCDEFGHJKLMNPQRSTUVWXYZ";
const DIGITS: &str = "23456789";
const SYMBOLS: &str = "!@#$%^&*()-_=+[]{};:,.?/";
const AMBIGUOUS: &str = "Il1O0o";

/// Настройки генератора. По умолчанию — 20 символов, все классы,
/// без визуально неоднозначных символов.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeneratorOptions {
    pub length: usize,
    pub lowercase: bool,
    pub uppercase: bool,
    pub digits: bool,
    pub symbols: bool,
    pub avoid_ambiguous: bool,
}

impl Default for GeneratorOptions {
    fn default() -> Self {
        Self {
            length: 20,
            lowercase: true,
            uppercase: true,
            digits: true,
            symbols: true,
            avoid_ambiguous: true,
        }
    }
}

impl GeneratorOptions {
    fn classes(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.lowercase {
            out.push(LOWER);
        }
        if self.uppercase {
            out.push(UPPER);
        }
        if self.digits {
            out.push(DIGITS);
        }
        if self.symbols {
            out.push(SYMBOLS);
        }
        out
    }

    /// Алфавит генератора с учётом фильтра визуально неоднозначных символов.
    pub fn pool_size(&self) -> usize {
        let mut charset = String::new();
        for class in &self.classes() {
            charset.push_str(class);
        }
        charset
            .chars()
            .filter(|c| !self.avoid_ambiguous || !AMBIGUOUS.contains(*c))
            .count()
    }

    /// Энтропия пароля, который выдаст генератор с такими настройками, в битах.
    pub fn entropy_bits(&self) -> f64 {
        let pool = self.pool_size();
        if pool == 0 {
            return 0.0;
        }
        self.length as f64 * (pool as f64).log2()
    }
}

/// Генерирует пароль, гарантируя хотя бы один символ из каждого включённого класса.
pub fn generate_password(opts: &GeneratorOptions) -> Result<String> {
    let classes = opts.classes();
    if classes.is_empty() {
        return Err(Error::InvalidFormat(
            "включите хотя бы один класс символов".into(),
        ));
    }
    if opts.length < classes.len() {
        return Err(Error::InvalidFormat(format!(
            "длина {} меньше числа классов символов {}",
            opts.length,
            classes.len()
        )));
    }

    let mut charset = String::new();
    for class in &classes {
        charset.push_str(class);
    }
    let charset: Vec<char> = if opts.avoid_ambiguous {
        charset
            .chars()
            .filter(|c| !AMBIGUOUS.contains(*c))
            .collect()
    } else {
        charset.chars().collect()
    };

    let mut rng = rng();
    let mut chars: Vec<char> = Vec::with_capacity(opts.length);

    // По одному обязательному символу из каждого класса.
    for class in &classes {
        let pool: Vec<char> = class
            .chars()
            .filter(|c| !opts.avoid_ambiguous || !AMBIGUOUS.contains(*c))
            .collect();
        if pool.is_empty() {
            continue;
        }
        chars.push(pool[rng.random_range(0..pool.len())]);
    }

    while chars.len() < opts.length {
        chars.push(charset[rng.random_range(0..charset.len())]);
    }

    // Перемешивание Фишера—Йетса, чтобы обязательные символы не стояли в начале.
    for i in (1..chars.len()).rev() {
        let j = rng.random_range(0..=i);
        chars.swap(i, j);
    }

    Ok(chars.into_iter().collect())
}

/// Оценка энтропии произвольного пароля в битах — по длине и классам символов.
/// Для сгенерированных случайных паролей и фраз оценка честная; пароль,
/// набранный человеком (слова, даты), реальной стойкости не имеет, но по
/// классам она и не прикидывается словарной атакой — это ориентир, не гарантия.
pub fn password_entropy_bits(password: &str) -> f64 {
    let len = password.chars().count();
    if len == 0 {
        return 0.0;
    }

    let mut ascii_lower = false;
    let mut ascii_upper = false;
    let mut digit = false;
    let mut symbol = false;
    let mut cyrillic = false;
    let mut other = false;
    for ch in password.chars() {
        if ch.is_ascii_lowercase() {
            ascii_lower = true;
        } else if ch.is_ascii_uppercase() {
            ascii_upper = true;
        } else if ch.is_ascii_digit() {
            digit = true;
        } else if matches!(ch, 'а'..='я' | 'ё' | 'А'..='Я' | 'Ё') {
            cyrillic = true;
        } else if !ch.is_ascii_alphanumeric() && ch.is_ascii() {
            symbol = true;
        } else {
            other = true;
        }
    }

    let mut pool = 0;
    if ascii_lower {
        pool += 26;
    }
    if ascii_upper {
        pool += 26;
    }
    if digit {
        pool += 10;
    }
    if symbol {
        pool += 32;
    }
    if cyrillic {
        pool += 66;
    }
    if other {
        pool += 100;
    }
    if pool == 0 {
        return 0.0;
    }
    len as f64 * (pool as f64).log2()
}
