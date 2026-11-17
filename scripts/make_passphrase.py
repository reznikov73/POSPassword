#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Собирает crates/vault-core/src/passphrase.rs из словарей и шаблона."""

RU = open("/home/z/my-project/scripts/words_ru.txt", encoding="utf-8").read().split()
EN = open("/home/z/my-project/scripts/words_en.txt", encoding="utf-8").read().split()


def array(name, words, per_line=8):
    lines = []
    for i in range(0, len(words), per_line):
        chunk = words[i:i + per_line]
        lines.append("    " + " ".join(f'"{w}",' for w in chunk))
    return f"pub(crate) const {name}: &[&str] = &[\n" + "\n".join(lines) + "\n];\n"


TEMPLATE = '''//! Запоминаемые пароли: фразы из случайных слов.

use rand::{RngExt, rng};

use crate::{{Error, Result}};

/// Русские слова — основа запоминаемого пароля по умолчанию.
pub const MIN_WORDS: usize = 3;
pub const MAX_WORDS: usize = 8;
pub const MAX_DIGITS: usize = 4;

__RU__

/// Английские слова — для сайтов, которые не принимают кириллицу.
__EN__

/// Язык слов во фразе.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wordlist {{
    Russian,
    English,
}}

impl Wordlist {{
    pub fn label(self) -> &'static str {{
        match self {{
            Wordlist::Russian => "Русские слова",
            Wordlist::English => "Латиницей",
        }}
    }}

    pub fn words(self) -> &'static [&'static str] {{
        match self {{
            Wordlist::Russian => RU_WORDS,
            Wordlist::English => EN_WORDS,
        }}
    }}

    pub fn all() -> [Wordlist; 2] {{
        [Wordlist::Russian, Wordlist::English]
    }}
}}

/// Разделитель между словами фразы.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Separator {{
    Hyphen,
    Dot,
    Space,
    Empty,
}}

impl Separator {{
    pub fn label(self) -> &'static str {{
        match self {{
            Separator::Hyphen => "дефис",
            Separator::Dot => "точка",
            Separator::Space => "пробел",
            Separator::Empty => "без разделителя",
        }}
    }}

    pub fn as_char(self) -> Option<char> {{
        match self {{
            Separator::Hyphen => Some('-'),
            Separator::Dot => Some('.'),
            Separator::Space => Some(' '),
            Separator::Empty => None,
        }}
    }}

    pub fn all() -> [Separator; 4] {{
        [Separator::Hyphen, Separator::Dot, Separator::Space, Separator::Empty]
    }}
}}

/// Настройки фразы. По умолчанию — 5 русских слов с заглавной буквы,
/// дефис-разделитель и две цифры: «Корабль-Мандарин-Вишня-Сосна-42».
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassphraseOptions {{
    pub words: usize,
    pub digits: usize,
    pub separator: Separator,
    pub capitalize: bool,
    pub language: Wordlist,
}}

impl Default for PassphraseOptions {{
    fn default() -> Self {{
        Self {{
            words: 5,
            digits: 2,
            separator: Separator::Hyphen,
            capitalize: true,
            language: Wordlist::Russian,
        }}
    }}
}}

impl PassphraseOptions {{
    fn validate(&self) -> Result<()> {{
        if self.words < MIN_WORDS || self.words > MAX_WORDS {{
            return Err(Error::InvalidFormat(format!(
                "слов во фразе должно быть от {{MIN_WORDS}} до {{MAX_WORDS}}"
            )));
        }}
        if self.digits > MAX_DIGITS {{
            return Err(Error::InvalidFormat(format!(
                "цифр не может быть больше {{MAX_DIGITS}}"
            )));
        }}
        Ok(())
    }}
}}

/// Генерирует запоминаемую фразу. Слова не повторяются, цифры дописываются
/// в конец: «Корабль-Мандарин-Вишня-Сосна-42».
pub fn generate_passphrase(opts: &PassphraseOptions) -> Result<String> {{
    opts.validate()?;
    let dict = opts.language.words();
    if opts.words > dict.len() {{
        return Err(Error::InvalidFormat(format!(
            "во словаре всего {{}} слов — столько не набрать",
            dict.len()
        )));
    }}

    let mut rng = rng();
    let mut used = vec![false; dict.len()];
    let mut chosen: Vec<&str> = Vec::with_capacity(opts.words);
    while chosen.len() < opts.words {{
        let idx = rng.random_range(0..dict.len());
        if used[idx] {{
            continue;
        }}
        used[idx] = true;
        chosen.push(dict[idx]);
    }}

    let words: Vec<String> = chosen
        .into_iter()
        .map(|word| {
            if opts.capitalize {{
                capitalize_first(word)
            }} else {{
                word.to_string()
            }}
        })
        .collect();

    let separator = opts
        .separator
        .as_char()
        .map(|ch| ch.to_string())
        .unwrap_or_default();
    let mut phrase = words.join(&separator);

    if opts.digits > 0 {{
        let mut digits = String::with_capacity(opts.digits);
        for _ in 0..opts.digits {{
            digits.push((b'0' + rng.random_range(0..10)) as char);
        }}
        if !phrase.is_empty() && !separator.is_empty() {{
            phrase.push_str(&separator);
        }}
        phrase.push_str(&digits);
    }

    Ok(phrase)
}}

/// Честная оценка энтропии фразы в битах: слова выбираются без повторов,
/// поэтому каждый следующий выбор чуть беднее предыдущего.
pub fn passphrase_entropy_bits(opts: &PassphraseOptions) -> f64 {{
    let n = opts.language.words().len() as f64;
    let words: f64 = (0..opts.words)
        .map(|i| (n - i as f64).max(1.0).log2())
        .sum();
    let digits = opts.digits as f64 * 10.0_f64.log2();
    words + digits
}}

fn capitalize_first(word: &str) -> String {{
    let mut chars = word.chars();
    match chars.next() {{
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }}
}}

#[cfg(test)]
mod tests {{
    use super::*;

    #[test]
    fn dictionaries_have_no_duplicates() {{
        for dict in [RU_WORDS, EN_WORDS] {{
            let mut sorted = dict.to_vec();
            sorted.sort_unstable();
            let before = sorted.len();
            sorted.dedup();
            assert_eq!(
                before,
                sorted.len(),
                "в словаре есть повторяющиеся слова"
            );
            assert!(dict.len() >= 512, "словарь подозрительно мал");
        }}
    }}

    #[test]
    fn passphrase_shape_matches_options() {{
        let opts = PassphraseOptions {{
            words: 4,
            digits: 2,
            separator: Separator::Hyphen,
            capitalize: true,
            language: Wordlist::Russian,
        }};
        let phrase = generate_passphrase(&opts).expect("фраза");
        let (text, digits) = phrase.rsplit_once('-').expect("цифры в конце");
        assert_eq!(digits.len(), 2);
        assert!(digits.chars().all(|ch| ch.is_ascii_digit()));
        let words: Vec<&str> = text.split('-').collect();
        assert_eq!(words.len(), 4);
        for word in words {{
            assert!(word.chars().next().expect("не пусто").is_uppercase());
        }}
    }}

    #[test]
    fn separator_and_capitalization_are_respected() {{
        let opts = PassphraseOptions {{
            words: 3,
            digits: 0,
            separator: Separator::Dot,
            capitalize: false,
            language: Wordlist::English,
        }};
        let phrase = generate_passphrase(&opts).expect("фраза");
        assert_eq!(phrase.split('.').count(), 3);
        assert!(phrase.chars().all(|ch| !ch.is_uppercase()));

        let no_sep = PassphraseOptions {{
            words: 3,
            digits: 0,
            separator: Separator::Empty,
            capitalize: false,
            language: Wordlist::English,
        }};
        let phrase = generate_passphrase(&no_sep).expect("фраза");
        assert!(!phrase.contains(' ') && !phrase.contains('-'));
    }}

    #[test]
    fn entropy_is_reasonable() {{
        let opts = PassphraseOptions::default();
        let bits = passphrase_entropy_bits(&opts);
        // 5 слов из ~1024 (по 10 бит) + 2 цифры (6,6 бит) ≈ 56 бит.
        assert!((45.0..=70.0).contains(&bits), "получилось {bits} бит");
    }}

    #[test]
    fn invalid_options_are_rejected() {{
        let mut opts = PassphraseOptions::default();
        opts.words = 2;
        assert!(generate_passphrase(&opts).is_err());
        opts.words = MAX_WORDS + 1;
        assert!(generate_passphrase(&opts).is_err());
        opts = PassphraseOptions::default();
        opts.digits = MAX_DIGITS + 1;
        assert!(generate_passphrase(&opts).is_err());
    }}
}}
'''

src = (TEMPLATE
       .replace("__RU__", array("RU_WORDS", RU).rstrip())
       .replace("__EN__", array("EN_WORDS", EN).rstrip())
       .replace("{{", "{")
       .replace("}}", "}"))

out = "/home/z/my-project/POSPassword/crates/vault-core/src/passphrase.rs"
with open(out, "w", encoding="utf-8") as f:
    f.write(src)
print(f"Записан {out}: RU={len(RU)}, EN={len(EN)}")
