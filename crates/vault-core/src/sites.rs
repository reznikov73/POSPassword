//! Матчинг сайтов: нормализация адресов, базовый домен и сравнение.
//!
//! Запись может относиться к нескольким сайтам (`url` плюс список `urls`),
//! а запросы приходят как от браузера (точный адрес вкладки), так и от
//! автоввода (заголовок окна, где домен встречается среди слов). Оба случая
//! сводятся к одному сравнению: совпадает ли адрес записи с адресом запроса.
//!
//! Сравнение «по базовому домену» (eTLD+1) устроено как у KeePassXC и
//! Bitwarden: `lk.domclick.ru` и `domclick.ru` — один сайт, а
//! `github.example.com` для `github.com` — чужой. Полного списка публичных
//! суффиксов (PSL) в офлайн-программу тащить нельзя, поэтому используется
//! компактный список двусоставных суффиксов (`co.uk`, `com.ru` и подобные).

/// Насколько адрес записи похож на то, что видно в заголовке окна.
/// Сортировка по убыванию: сначала точные совпадения, потом догадки по слову.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchStrength {
    /// Ничего общего.
    None,
    /// В заголовке окна встречается имя сайта без зоны (например, «GitHub»).
    Weak,
    /// В заголовке окна встречается сам домен или базовый домен сайта.
    Strong,
}

/// Двусоставные публичные суффиксы, для которых базовым доменом считаются
/// три последние метки (`a.b.co.uk` → `b.co.uk`). Без полного PSL список
/// покрывает большинство реально встречающихся сайтов; прочие зоны режутся
/// по простому правилу «две последние метки», как у KeePassXC.
const SECOND_LEVEL_SUFFIXES: [&str; 49] = [
    // Великобритания
    "co.uk", "org.uk", "ac.uk", "gov.uk", "net.uk", "sch.uk", "me.uk",
    // Россия и СНГ
    "com.ru", "net.ru", "org.ru", "edu.ru", "gov.ru", "int.ru", "com.ua", "net.ua", "org.ua",
    "gov.ua", "in.ua", "com.kz", "org.kz", "net.kz", "gov.kz", "edu.kz",
    // Япония, Корея, Китай
    "co.jp", "or.jp", "ne.jp", "ac.jp", "go.jp", "co.kr", "or.kr", "com.cn", "net.cn", "org.cn",
    "gov.cn", "edu.cn",
    // Австралия, Новая Зеландия, Индия
    "com.au", "net.au", "org.au", "edu.au", "gov.au", "co.nz", "co.in", "net.in", "org.in",
    "gov.in", // Америка
    "com.br", "com.mx", "com.ar", "co.za",
];

/// Выделяет хост из адреса или «голого» домена: срезает схему, пользователя,
/// порт и путь, убирает «www.». Возвращает пустой `None` для мусора.
///
/// ```text
/// https://lk.domclick.ru/path?x=1  → lk.domclick.ru
/// www.Mail.RU                     → mail.ru
/// user:pass@site.ru:8443/x        → site.ru
/// localhost:8080                  → localhost
/// ```
pub fn normalize_host(input: &str) -> Option<String> {
    let mut rest = input.trim().to_lowercase();

    // Срезаем схему: она идёт до «://» и в ней тоже есть «/».
    if let Some(pos) = rest.find("://") {
        rest.drain(..pos + 3);
    }
    // Теперь всё после первого разделителя пути — мусор.
    if let Some(pos) = rest.find(['/', '?', '#']) {
        rest.truncate(pos);
    }
    // Информация пользователя: до «@».
    if let Some(pos) = rest.rfind('@') {
        rest.drain(..=pos);
    }
    // Порт: последний «:», за которым только цифры (IPv6 не поддерживаем —
    // в парольных записях такие адреса не встречаются).
    if let Some((head, tail)) = rest.rsplit_once(':')
        && !tail.is_empty()
        && tail.chars().all(|ch| ch.is_ascii_digit())
    {
        rest = head.to_string();
    }
    // «www.» не различает сайтов.
    if let Some(stripped) = rest.strip_prefix("www.") {
        rest = stripped.to_string();
    }

    if rest.is_empty() || rest.contains(' ') {
        return None;
    }
    Some(rest)
}

/// Базовый домен (приближение к eTLD+1): `lk.domclick.ru` → `domclick.ru`,
/// `a.b.co.uk` → `b.co.uk`, `localhost` и IP-адреса остаются как есть.
pub fn registrable_domain(host: &str) -> String {
    let host = host.trim().to_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() <= 2 {
        return host;
    }
    // IP-адрес (хоть одна метка — только цифры) не сворачиваем.
    if labels
        .iter()
        .any(|label| label.chars().all(|ch| ch.is_ascii_digit()))
    {
        return host;
    }
    let second_level = format!("{}.{}", labels[labels.len() - 2], labels[labels.len() - 1]);
    if SECOND_LEVEL_SUFFIXES.contains(&second_level.as_str()) {
        labels[labels.len() - 3..].join(".")
    } else {
        labels[labels.len() - 2..].join(".")
    }
}

/// Одинаков ли сайт у двух адресов: точно тот же хост или тот же базовый домен.
pub fn url_matches(left: &str, right: &str) -> bool {
    let (Some(left), Some(right)) = (normalize_host(left), normalize_host(right)) else {
        return false;
    };
    if left == right {
        return true;
    }
    registrable_domain(&left) == registrable_domain(&right)
}

/// Похоже ли, что заголовок окна относится к сайту записи: точное совпадение
/// домена сильное, а имя сайта без зоны («GitHub») — лишь догадка.
pub fn url_matches_title(url: &str, title: &str) -> MatchStrength {
    let Some(host) = normalize_host(url) else {
        return MatchStrength::None;
    };
    let title = title.to_lowercase();
    if title.contains(&host) {
        return MatchStrength::Strong;
    }
    let base = registrable_domain(&host);
    if !base.is_empty() && title.contains(&base) {
        return MatchStrength::Strong;
    }
    // Имя сайта без зоны — только если оно достаточно длинное, чтобы не
    // ловить «mail» в каждом втором заголовке, и стоит отдельным словом.
    if let Some(name) = base.split('.').next()
        && name.chars().count() >= 4
        && title
            .split(|ch: char| !ch.is_alphanumeric())
            .any(|word| word == name)
    {
        return MatchStrength::Weak;
    }
    MatchStrength::None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_urls_and_bare_domains() {
        assert_eq!(
            normalize_host("https://lk.domclick.ru/path?x=1"),
            Some("lk.domclick.ru".to_string())
        );
        assert_eq!(normalize_host("www.Mail.RU"), Some("mail.ru".to_string()));
        assert_eq!(
            normalize_host("https://user:pass@site.ru:8443/x"),
            Some("site.ru".to_string())
        );
        assert_eq!(normalize_host("site.ru:8443"), Some("site.ru".to_string()));
        assert_eq!(
            normalize_host("localhost:8080"),
            Some("localhost".to_string())
        );
        assert_eq!(normalize_host(""), None);
        assert_eq!(normalize_host("https://"), None);
        assert_eq!(normalize_host("не адрес вовсе"), None);
    }

    #[test]
    fn registrable_domain_folds_subdomains_and_two_part_suffixes() {
        assert_eq!(registrable_domain("lk.domclick.ru"), "domclick.ru");
        assert_eq!(registrable_domain("a.b.co.uk"), "b.co.uk");
        assert_eq!(registrable_domain("mail.ru"), "mail.ru");
        assert_eq!(registrable_domain("localhost"), "localhost");
        assert_eq!(registrable_domain("192.168.1.5"), "192.168.1.5");
        assert_eq!(registrable_domain("www.github.com"), "github.com");
    }

    #[test]
    fn same_site_matching_subdomains_and_www() {
        assert!(url_matches("https://lk.domclick.ru/login", "domclick.ru"));
        assert!(url_matches("www.github.com", "https://github.com/features"));
        assert!(url_matches("https://mail.ru", "https://mail.ru:443/inbox"));
    }

    #[test]
    fn lookalike_domain_is_not_the_same_site() {
        // Защита от фишинга: базовые домены разные — сайт чужой.
        assert!(!url_matches(
            "https://github.example.com",
            "https://github.com"
        ));
        assert!(!url_matches(
            "https://sberbank-online.ru",
            "https://online.sberbank.ru"
        ));
        assert!(!url_matches("https://mail.ru", "https://mail.com"));
    }

    #[test]
    fn title_with_domain_is_a_strong_match() {
        assert_eq!(
            url_matches_title("https://mail.ru", "Почта Mail.ru — вход"),
            MatchStrength::Strong
        );
        assert_eq!(
            url_matches_title(
                "https://online.sberbank.ru",
                "Сбербанк Онлайн (online.sberbank.ru)"
            ),
            MatchStrength::Strong
        );
    }

    #[test]
    fn title_with_site_name_is_a_weak_match() {
        // «github» — отдельное слово, но зоны в заголовке нет.
        assert_eq!(
            url_matches_title("https://github.com", "GitHub: let's build from here"),
            MatchStrength::Weak
        );
        // Короткие имена без зоны не считаем совпадением.
        assert_eq!(
            url_matches_title("https://vk.com", "Одноклассники — вход"),
            MatchStrength::None
        );
    }

    #[test]
    fn word_boundary_keeps_lookalikes_apart() {
        // «mail» внутри «mail.ru» — это другой сайт, но слово то же:
        // такое совпадение остаётся слабой догадкой, а не точным попаданием.
        assert_eq!(
            url_matches_title("https://mail.com", "Почта Mail.ru — вход"),
            MatchStrength::Weak
        );
        // Части слова не подходят: «сбербанк» ≠ «сбер».
        assert_eq!(
            url_matches_title("https://sber.ru", "Сбербанк Онлайн"),
            MatchStrength::None
        );
    }

    #[test]
    fn item_urls_cover_primary_and_extra_sites() {
        use crate::ItemKind;
        use crate::model::Item;

        let mut item = Item::new(ItemKind::Login, "Домклик");
        item.url = "https://lk.domclick.ru".to_string();
        item.urls = vec!["domclick.ru".to_string(), "".to_string()];
        assert!(item.matches_url("https://domclick.ru/about"));
        assert!(!item.matches_url("https://mail.ru"));

        let mut other = Item::new(ItemKind::Login, "GitHub");
        other.url = String::new();
        other.urls = vec!["github.com".to_string()];
        assert!(other.matches_url("https://gist.github.com"));

        // Старые файлы без списка сайтов читаются: поле просто пустое.
        let json = serde_json::json!({
            "id": item.id, "kind": "login", "title": "T",
            "created_at": 0, "updated_at": 0
        });
        let parsed: Item = serde_json::from_value(json).expect("старый формат");
        assert!(parsed.urls.is_empty());
        assert!(parsed.all_urls().is_empty());
    }

    #[test]
    fn search_also_looks_at_extra_sites() {
        use crate::ItemKind;
        use crate::model::Item;

        let mut item = Item::new(ItemKind::Login, "Тинькофф");
        item.url = String::new();
        item.urls = vec!["tinkoff.ru".to_string()];
        assert!(item.matches("tinkoff"));
    }
}
