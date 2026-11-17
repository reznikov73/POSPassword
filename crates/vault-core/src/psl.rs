//! Полный список публичных суффиксов (PSL), встроенный в бинарник.
//!
//! Снапшот официального списка <https://publicsuffix.org/list/> лежит рядом
//! (`psl_suffixes.dat`) и парсится при первом обращении. Приватные зоны
//! (`github.io`, `herokuapp.com` и подобные) включены: `userA.github.io` и
//! `userB.github.io` — разные сайты.
//!
//! Обновление снапшота: скачать свежий список в `src/psl_suffixes.dat`
//! (скрипт `tools/update-psl.ps1`), тесты `sites.rs` ловят регрессии.
//! Старый снапшот не опасен: правило неизвестного TLD не зависит от
//! свежести, а новые суффиксы лишь уточняют разрезание.

use std::collections::HashSet;
use std::sync::OnceLock;

/// Суффиксы-исключения (`!www.ck`) без восклицательного знака.
static EXCEPTIONS: OnceLock<HashSet<String>> = OnceLock::new();
/// Обычные правила (`co.uk`).
static EXACT: OnceLock<HashSet<String>> = OnceLock::new();
/// Часть после звёздочки (`*.ck` → `ck`).
static WILDCARDS: OnceLock<HashSet<String>> = OnceLock::new();

/// Разобранный список.
struct Rules {
    exceptions: &'static HashSet<String>,
    exact: &'static HashSet<String>,
    wildcards: &'static HashSet<String>,
}

fn rules() -> Rules {
    fn parse() -> (
        &'static HashSet<String>,
        &'static HashSet<String>,
        &'static HashSet<String>,
    ) {
        let mut exceptions = HashSet::new();
        let mut exact = HashSet::new();
        let mut wildcards = HashSet::new();
        for line in include_str!("psl_suffixes.dat").lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("//") {
                continue;
            }
            // Нормализуем в punycode: в списке встречаются и unicode-записи.
            let ascii = punycode(line);
            if let Some(rule) = ascii.strip_prefix('!') {
                exceptions.insert(rule.to_string());
            } else if let Some(rule) = ascii.strip_prefix("*.") {
                wildcards.insert(rule.to_string());
            } else {
                exact.insert(ascii);
            }
        }
        let _ = (
            EXCEPTIONS.set(exceptions),
            EXACT.set(exact),
            WILDCARDS.set(wildcards),
        );
        (
            EXCEPTIONS.get().unwrap(),
            EXACT.get().unwrap(),
            WILDCARDS.get().unwrap(),
        )
    }
    // Один поток исполняет parse, остальные ждут — то же самое делает
    // get_or_init, но здесь нужно вернуть три сета сразу.
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = parse();
    });
    Rules {
        exceptions: EXCEPTIONS.get().unwrap(),
        exact: EXACT.get().unwrap(),
        wildcards: WILDCARDS.get().unwrap(),
    }
}

/// Unicode-домен → punycode (ASCII); ASCII остаётся как есть.
fn punycode(domain: &str) -> String {
    if domain.is_ascii() {
        return domain.to_lowercase();
    }
    idna::domain_to_ascii(domain).unwrap_or_else(|_| domain.to_lowercase())
}

/// Число меток в публичном суффиксе домена по алгоритму PSL.
///
/// Если ни одно правило не подошло, действует неявное «*»: суффикс —
/// последняя метка. Исключения (`!www.ck`) перекрывают остальные правила:
/// из правила снимается крайняя левая метка.
pub(crate) fn public_suffix_len(labels: &[&str]) -> usize {
    let rules = rules();

    // Исключения: действует самое длинное подошедшее.
    let mut exception_len = 0;
    for n in 1..=labels.len() {
        let candidate = labels[labels.len() - n..].join(".");
        if rules.exceptions.contains(&candidate) {
            exception_len = n;
        }
    }
    if exception_len > 0 {
        // Из правила снимается левая метка: суффикс короче на одну.
        return exception_len - 1;
    }

    let mut best = 0;
    for n in 1..=labels.len() {
        let candidate = labels[labels.len() - n..].join(".");
        if rules.exact.contains(&candidate) {
            best = best.max(n);
        }
        // Правило «*.b.c» подходит доменам «x.b.c»: суффикс длиннее на метку.
        if rules.wildcards.contains(&candidate) && labels.len() > n {
            best = best.max(n + 1);
        }
    }
    // Неявное правило «*»: неизвестный TLD — суффикс из одной метки.
    best.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suffix_len(host: &str) -> usize {
        let labels: Vec<&str> = host.split('.').collect();
        public_suffix_len(&labels)
    }

    #[test]
    fn plain_tld_is_one_label() {
        assert_eq!(suffix_len("lk.domclick.ru"), 1);
        assert_eq!(suffix_len("example.unknown-tld"), 1);
    }

    #[test]
    fn two_part_suffixes_count_two_labels() {
        assert_eq!(suffix_len("a.b.co.uk"), 2);
        assert_eq!(suffix_len("bank.co.il"), 2);
    }

    #[test]
    fn idn_tld_is_punycoded() {
        // «рф» в списке — как punycode-правило xn--p1ai.
        assert_eq!(suffix_len("тест.xn--p1ai"), 1);
    }

    #[test]
    fn private_domains_are_rules_too() {
        assert_eq!(suffix_len("user.github.io"), 2);
        assert_eq!(suffix_len("app.herokuapp.com"), 2);
    }

    #[test]
    fn wildcard_rules_match_one_label() {
        // *.ck: суффикс foo.ck — две метки.
        assert_eq!(suffix_len("foo.ck"), 2);
        assert_eq!(suffix_len("a.b.ck"), 2);
    }

    #[test]
    fn exception_rules_shrink_suffix() {
        // !www.ck: для www.ck суффикс — ck, одна метка.
        assert_eq!(suffix_len("www.ck"), 1);
    }

    #[test]
    fn parsed_list_is_not_empty() {
        let rules = rules();
        assert!(rules.exact.len() > 5_000, "exact: {}", rules.exact.len());
        assert!(!rules.wildcards.is_empty());
        assert!(!rules.exceptions.is_empty());
    }
}
