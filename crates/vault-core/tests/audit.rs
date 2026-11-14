use vault_core::{Item, ItemKind, audit, expiries, is_expiry_label, parse_date};

/// 2027-01-01 00:00:00 UTC.
const NOW: i64 = 1_798_761_600;
const DAY: i64 = 86_400;

fn login(title: &str, password: &str) -> Item {
    let mut item = Item::new(ItemKind::Login, title);
    item.username = "user@example.com".into();
    item.password = password.into();
    item
}

#[test]
fn weak_passwords_are_reported() {
    let report = audit(
        &[
            login("Короткий", "123456"),
            login("Длинный", "длинный-пароль-2026"),
        ],
        NOW,
    );
    assert_eq!(report.weak.len(), 1);
    assert_eq!(report.weak[0].title, "Короткий");
    assert!(report.weak[0].detail.contains('6'));
}

#[test]
fn reused_passwords_are_reported() {
    let report = audit(
        &[
            login("Почта", "одинаковый-пароль"),
            login("Форум", "одинаковый-пароль"),
            login("Банк", "уникальный-пароль"),
        ],
        NOW,
    );
    assert_eq!(report.reused.len(), 2);
    assert!(
        report
            .reused
            .iter()
            .all(|finding| finding.detail.contains('2'))
    );
}

#[test]
fn old_records_are_reported() {
    let mut item = login("Старый", "длинный-пароль-2026");
    item.updated_at = NOW - 400 * DAY;
    let report = audit(&[item], NOW);
    assert_eq!(report.old.len(), 1);
    assert!(report.old[0].detail.contains("400"));
}

#[test]
fn expiring_document_is_reported() {
    let mut item = Item::with_template(ItemKind::MedicalPolicy, "Полис ОМС");
    item.set_field("Срок действия", "12.03.2027");

    let report = audit(&[item], NOW);
    assert_eq!(report.expiring.len(), 1);
    assert!(report.expiring[0].detail.contains("70"));
    assert!(report.expiring[0].detail.contains("Срок действия"));
}

#[test]
fn overdue_document_is_reported() {
    let mut item = Item::with_template(ItemKind::ForeignPassport, "Загранпаспорт");
    item.set_field("Срок действия", "01.01.2020");

    let report = audit(&[item], NOW);
    assert_eq!(report.expiring.len(), 1);
    assert!(report.expiring[0].detail.contains("просрочен"));
}

#[test]
fn far_future_expiry_is_not_reported() {
    let mut item = Item::with_template(ItemKind::DriverLicense, "Права");
    item.set_field("Срок действия", "01.01.2030");
    let report = audit(&[item], NOW);
    assert!(report.expiring.is_empty());
}

#[test]
fn expiries_reports_days_left() {
    let mut item = Item::with_template(ItemKind::Card, "Карта");
    item.set_field("Срок действия", "12/27");
    let found = expiries(&item, NOW);
    assert_eq!(found.len(), 1);
    // До 31.12.2027 от 01.01.2027 — 364 дня.
    assert_eq!(found[0].days_left, 364);
}

#[test]
fn parse_date_supports_common_formats() {
    let dot = parse_date("12.03.2027").expect("точки");
    assert_eq!(
        (dot.year(), u8::from(dot.month()), dot.day()),
        (2027, 3, 12)
    );

    let iso = parse_date("2027-03-12").expect("ISO");
    assert_eq!(
        (iso.year(), u8::from(iso.month()), iso.day()),
        (2027, 3, 12)
    );

    let slash = parse_date("12/03/2027").expect("слэши");
    assert_eq!(
        (slash.year(), u8::from(slash.month()), slash.day()),
        (2027, 3, 12)
    );

    let card = parse_date("12/27").expect("срок карты");
    assert_eq!(
        (card.year(), u8::from(card.month()), card.day()),
        (2027, 12, 31)
    );

    let with_suffix = parse_date("до 12.03.2027 г.").expect("с префиксом");
    assert_eq!(with_suffix.year(), 2027);

    assert!(parse_date("бессрочно").is_none());
    assert!(parse_date("123456").is_none());
}

#[test]
fn expiry_labels_are_recognized() {
    assert!(is_expiry_label("Срок действия"));
    assert!(is_expiry_label("ОСАГО до"));
    assert!(is_expiry_label("Техосмотр до"));
    assert!(is_expiry_label("Действителен до"));
    assert!(!is_expiry_label("Серия и номер"));
    assert!(!is_expiry_label("Кем выдан"));
}

#[test]
fn empty_report_for_clean_vault() {
    let mut item = Item::new(ItemKind::SecureNote, "Заметка");
    item.password = "длинный-пароль-2026".into();
    let report = audit(&[item], NOW);
    assert!(report.is_empty());
    assert_eq!(report.total(), 0);
}
