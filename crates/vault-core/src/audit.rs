//! Аудит хранилища: слабые и повторяющиеся пароли, давно не обновлённые
//! записи, истекающие документы, карты и полисы, а также эвристики
//! очевидных паролей: пароль равен логину, содержит имя сайта или входит
//! в словарь самых употребимых паролей.
//!
//! Всё считается локально по расшифрованным записям: ни один пароль
//! не покидает машину и не показывается в отчёте.

use std::collections::HashSet;
use std::sync::OnceLock;

use time::Date;
use time::OffsetDateTime;
use time::macros::format_description;
use uuid::Uuid;

use crate::model::{Item, ItemKind};
use crate::sites::normalize_host;

/// Словарь самых употребимых паролей (топ-10 000, SecLists, MIT).
/// Встроен в бинарник: проверка офлайновая, в поиск и экспорт не попадает.
const COMMON_PASSWORDS: &str = include_str!("passwords_top10000.txt");

static COMMON_SET: OnceLock<HashSet<&'static str>> = OnceLock::new();

/// Входит ли пароль в словарь самых употребимых (регистр не важен).
///
/// Используется аудитом записей и подсказкой стойкости мастер-пароля.
pub fn is_common_password(password: &str) -> bool {
    let set = COMMON_SET.get_or_init(|| {
        COMMON_PASSWORDS
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect()
    });
    set.contains(password.trim().to_lowercase().as_str())
}

/// Пароли короче этого считаем слабыми.
const WEAK_LEN: usize = 12;
/// Сколько дней запись считается давно не обновлённой.
const OLD_DAYS: i64 = 365;
/// За сколько дней до истечения начинаем предупреждать.
pub const EXPIRY_SOON_DAYS: i64 = 90;

/// Одна находка аудита. Секретных значений здесь нет — только факт и счётчик.
#[derive(Clone, Debug)]
pub struct Finding {
    pub id: Uuid,
    pub title: String,
    pub kind: ItemKind,
    pub detail: String,
}

/// Итог аудита. Пустые списки — это хорошо.
#[derive(Clone, Debug, Default)]
pub struct AuditReport {
    /// Истекающие и просроченные документы, карты, полисы.
    pub expiring: Vec<Finding>,
    /// Пароли, которые повторяются в нескольких записях.
    pub reused: Vec<Finding>,
    /// Короткие пароли.
    pub weak: Vec<Finding>,
    /// Записи, которые давно не обновлялись.
    pub old: Vec<Finding>,
    /// Пароль совпадает с логином записи.
    pub login_passwords: Vec<Finding>,
    /// Пароль содержит имя сайта или домен записи.
    pub site_passwords: Vec<Finding>,
    /// Пароль входит в словарь самых употребимых.
    pub common_passwords: Vec<Finding>,
}

impl AuditReport {
    pub fn total(&self) -> usize {
        self.expiring.len()
            + self.reused.len()
            + self.weak.len()
            + self.old.len()
            + self.login_passwords.len()
            + self.site_passwords.len()
            + self.common_passwords.len()
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
}

/// Срок действия конкретного поля записи.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expiry {
    pub label: String,
    /// Дней до даты: отрицательное — уже просрочено.
    pub days_left: i64,
}

/// Полный аудит на момент `now` (секунды UNIX).
pub fn audit(items: &[Item], now: i64) -> AuditReport {
    let mut report = AuditReport::default();

    let mut expiring: Vec<(i64, Finding)> = Vec::new();
    for item in items {
        for expiry in expiries(item, now) {
            if expiry.days_left > EXPIRY_SOON_DAYS {
                continue;
            }
            let detail = if expiry.days_left < 0 {
                format!("{}: просрочен на {} дн.", expiry.label, -expiry.days_left)
            } else {
                format!("{}: истекает через {} дн.", expiry.label, expiry.days_left)
            };
            expiring.push((
                expiry.days_left,
                Finding {
                    id: item.id,
                    title: item.title.clone(),
                    kind: item.kind,
                    detail,
                },
            ));
        }
    }
    expiring.sort_by_key(|(days, _)| *days);
    report.expiring = expiring.into_iter().map(|(_, finding)| finding).collect();

    // Повторяющиеся пароли: группируем по значению, сообщаем только счётчик.
    let mut by_password: Vec<(&str, Vec<&Item>)> = Vec::new();
    for item in items {
        if item.password.is_empty() {
            continue;
        }
        match by_password
            .iter_mut()
            .find(|(value, _)| *value == item.password)
        {
            Some((_, group)) => group.push(item),
            None => by_password.push((item.password.as_str(), vec![item])),
        }
    }
    let mut reused_groups: Vec<Vec<&Item>> = by_password
        .into_iter()
        .filter(|(_, group)| group.len() > 1)
        .map(|(_, group)| group)
        .collect();
    reused_groups.sort_by_key(|group| std::cmp::Reverse(group.len()));
    for group in reused_groups {
        let count = group.len();
        for item in group {
            report.reused.push(Finding {
                id: item.id,
                title: item.title.clone(),
                kind: item.kind,
                detail: format!("пароль повторяется в {count} записях"),
            });
        }
    }

    for item in items {
        let length = item.password.chars().count();
        if length > 0 && length < WEAK_LEN {
            report.weak.push(Finding {
                id: item.id,
                title: item.title.clone(),
                kind: item.kind,
                detail: format!("пароль из {length} символов"),
            });
        }
    }
    report
        .weak
        .sort_by(|left, right| left.title.cmp(&right.title));

    for item in items {
        let age = (now - item.updated_at) / 86_400;
        if age > OLD_DAYS {
            report.old.push(Finding {
                id: item.id,
                title: item.title.clone(),
                kind: item.kind,
                detail: format!("не обновлялась {age} дн."),
            });
        }
    }
    report
        .old
        .sort_by_key(|finding| std::cmp::Reverse(finding.detail.clone()));

    for item in items {
        if item.password.is_empty() {
            continue;
        }
        // Пароль равен логину: подставлять нечего, перебор тривиален.
        if !item.username.trim().is_empty()
            && item
                .password
                .trim()
                .eq_ignore_ascii_case(item.username.trim())
        {
            report.login_passwords.push(Finding {
                id: item.id,
                title: item.title.clone(),
                kind: item.kind,
                detail: "пароль совпадает с логином".into(),
            });
        }
        // Пароль содержит имя сайта или домен.
        if password_mentions_site(item) {
            report.site_passwords.push(Finding {
                id: item.id,
                title: item.title.clone(),
                kind: item.kind,
                detail: "пароль содержит имя сайта".into(),
            });
        }
        // Словарь самых употребимых паролей.
        if is_common_password(&item.password) {
            report.common_passwords.push(Finding {
                id: item.id,
                title: item.title.clone(),
                kind: item.kind,
                detail: "пароль входит в топ-10 000 употребимых".into(),
            });
        }
    }
    report.login_passwords.sort_by(|l, r| l.title.cmp(&r.title));
    report.site_passwords.sort_by(|l, r| l.title.cmp(&r.title));
    report
        .common_passwords
        .sort_by(|l, r| l.title.cmp(&r.title));

    report
}

/// Содержит ли пароль имя сайта записи: сам домен, домен без точек или
/// имя сайта без зоны (не короче четырёх символов, чтобы «mail» не ловил
/// каждое слово).
fn password_mentions_site(item: &Item) -> bool {
    let password = item.password.to_lowercase();
    if password.is_empty() {
        return false;
    }
    for url in item.all_urls() {
        let Some(host) = normalize_host(url) else {
            continue;
        };
        let base = crate::sites::registrable_domain(&host);
        if base.is_empty() {
            continue;
        }
        if password.contains(&base) {
            return true;
        }
        let no_dots = base.replace('.', "");
        if no_dots.len() >= 4 && password.contains(&no_dots) {
            return true;
        }
        if let Some(name) = base.split('.').next()
            && name.chars().count() >= 4
            && password.contains(name)
        {
            return true;
        }
    }
    false
}

/// Сроки действия полей записи, которые удалось разобрать как дату.
pub fn expiries(item: &Item, now: i64) -> Vec<Expiry> {
    let today = today_of(now);
    let mut found = Vec::new();
    for field in &item.fields {
        if !is_expiry_label(&field.label) {
            continue;
        }
        let Some(date) = parse_date(&field.value) else {
            continue;
        };
        found.push(Expiry {
            label: field.label.clone(),
            days_left: i64::from(date.to_julian_day()) - i64::from(today.to_julian_day()),
        });
    }
    found
}

/// Поля с такими названиями считаются сроками действия.
pub fn is_expiry_label(label: &str) -> bool {
    let lower = label.to_lowercase();
    lower.contains("срок")
        || lower.contains("действител")
        || lower.contains("осаго")
        || lower.contains("техосмотр")
}

/// Разбирает дату из поля: «12.03.2027», «2027-03-12», «12/03/2027»
/// и «12/27» для банковских карт (срок — конец месяца).
pub fn parse_date(value: &str) -> Option<Date> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let candidates = std::iter::once(trimmed).chain(trimmed.split_whitespace());
    for candidate in candidates {
        let cleaned = candidate
            .trim_matches(|ch: char| !(ch.is_ascii_digit() || ch == '.' || ch == '-' || ch == '/'));
        if cleaned.is_empty() {
            continue;
        }
        for format in DATE_FORMATS {
            if let Ok(date) = Date::parse(cleaned, format) {
                let year = date.year();
                if (1990..=2100).contains(&year) {
                    return Some(date);
                }
            }
        }
        if let Some(date) = parse_month_year(cleaned) {
            return Some(date);
        }
    }
    None
}

const DATE_FORMATS: &[&[time::format_description::FormatItem<'_>]] = &[
    format_description!("[day].[month].[year]"),
    format_description!("[year]-[month]-[day]"),
    format_description!("[day]/[month]/[year]"),
];

/// «12/27» — месяц и две последние цифры года; срок действует до конца месяца.
fn parse_month_year(value: &str) -> Option<Date> {
    let (month, year) = value.split_once('/')?;
    let month: u8 = month.trim().parse().ok()?;
    let year: i32 = year.trim().parse().ok()?;
    if !(1..=12).contains(&month) || !(0..=99).contains(&year) {
        return None;
    }
    let full_year = 2000 + year;
    let month = time::Month::try_from(month).ok()?;
    let day = month.length(full_year);
    Date::from_calendar_date(full_year, month, day).ok()
}

fn today_of(now: i64) -> Date {
    OffsetDateTime::from_unix_timestamp(now)
        .map(|moment| moment.date())
        .unwrap_or_else(|_| Date::from_calendar_date(1970, time::Month::January, 1).expect("эпоха"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Item;

    fn login(title: &str, username: &str, password: &str, url: &str) -> Item {
        let mut item = Item::new(ItemKind::Login, title);
        item.username = username.into();
        item.password = password.into();
        item.url = url.into();
        item
    }

    #[test]
    fn common_dictionary_is_recognized() {
        assert!(is_common_password("password"));
        assert!(!is_common_password("XyZzY-unlikely-9"));
        assert!(is_common_password("qwerty123") || is_common_password("Qwerty123"));
        assert!(
            !is_common_password("s3cret-GH-2026!"),
            "случайный пароль не должен быть в словаре"
        );
    }

    #[test]
    fn audit_flags_login_like_password() {
        let items = vec![login("Пара", "ivan", "ivan", "https://x.ru")];
        let report = audit(&items, 0);
        assert_eq!(report.login_passwords.len(), 1);
    }

    #[test]
    fn audit_flags_site_in_password() {
        let items = vec![login("Почта", "ivan", "mailru2026", "https://mail.ru")];
        let report = audit(&items, 0);
        assert_eq!(report.site_passwords.len(), 1, "почта с доменом в пароле");
        let clean = vec![login("Почта", "ivan", "s3cret-2026", "https://mail.ru")];
        assert!(audit(&clean, 0).site_passwords.is_empty());
    }

    #[test]
    fn audit_flags_common_password() {
        let items = vec![login("Сайт", "ivan", "password123", "https://x.ru")];
        let report = audit(&items, 0);
        assert_eq!(report.common_passwords.len(), 1);
    }

    #[test]
    fn audit_details_never_contain_passwords() {
        let items = vec![login(
            "Сайт",
            "user@example.com",
            "password123",
            "https://x.ru",
        )];
        let report = audit(&items, 0);
        for group in [
            &report.login_passwords,
            &report.site_passwords,
            &report.common_passwords,
        ] {
            for finding in group {
                assert!(
                    !finding.detail.contains("password123"),
                    "в отчёте не должно быть значения"
                );
            }
        }
    }
}
