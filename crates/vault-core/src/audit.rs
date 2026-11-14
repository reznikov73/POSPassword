//! Аудит хранилища: слабые и повторяющиеся пароли, давно не обновлённые
//! записи, истекающие документы, карты и полисы.
//!
//! Всё считается локально по расшифрованным записям: ни один пароль
//! не покидает машину и не показывается в отчёте.

use time::Date;
use time::OffsetDateTime;
use time::macros::format_description;
use uuid::Uuid;

use crate::model::{Item, ItemKind};

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
}

impl AuditReport {
    pub fn total(&self) -> usize {
        self.expiring.len() + self.reused.len() + self.weak.len() + self.old.len()
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

    report
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
