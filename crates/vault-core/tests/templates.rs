use std::collections::HashSet;

use vault_core::{Item, ItemKind};

#[test]
fn every_template_kind_has_fields() {
    for kind in ItemKind::all() {
        let fields = kind.template();
        if matches!(kind, ItemKind::Login | ItemKind::SecureNote) {
            assert!(
                fields.is_empty(),
                "у {} не должно быть шаблона",
                kind.label()
            );
        } else {
            assert!(
                !fields.is_empty(),
                "у типа «{}» нет ни одного поля",
                kind.label()
            );
        }
    }
}

#[test]
fn labels_are_unique_and_not_empty() {
    let mut seen = HashSet::new();
    for kind in ItemKind::all() {
        let label = kind.label();
        assert!(!label.trim().is_empty());
        assert!(seen.insert(label), "подпись «{label}» повторяется");
    }
}

#[test]
fn sensitive_fields_are_marked() {
    let passport = ItemKind::Passport.template();
    let number = passport
        .iter()
        .find(|f| f.label.contains("Серия и номер"))
        .expect("серия и номер");
    assert!(number.sensitive, "номер паспорта должен быть скрытым");

    let snils = ItemKind::Snils.template();
    assert!(snils[0].sensitive, "номер СНИЛС скрытый");

    let api = ItemKind::ApiKey.template();
    let key = api.iter().find(|f| f.label == "Ключ").expect("ключ");
    assert!(key.sensitive, "API-ключ скрытый");

    let pin = ItemKind::Pin.template();
    assert!(pin.iter().any(|f| f.label == "PIN" && f.sensitive));
    assert!(pin.iter().any(|f| f.label == "PUK" && f.sensitive));
}

#[test]
fn template_item_is_created_with_fields() {
    let item = Item::with_template(ItemKind::DriverLicense, "Моё ВУ");
    assert_eq!(item.kind, ItemKind::DriverLicense);
    assert!(item.field("Категории").is_some());
    assert!(item.field("Срок действия").is_some());
}

#[test]
fn kind_is_serialized_in_snake_case() {
    let item = Item::with_template(ItemKind::ForeignPassport, "Загран");
    let json = serde_json::to_string(&item).expect("сериализация");
    assert!(json.contains("\"foreign_passport\""));
}

#[test]
fn document_numbers_are_not_searchable() {
    let mut item = Item::with_template(ItemKind::Passport, "Паспорт");
    item.set_field("Серия и номер", "4510123456");
    item.set_field("Кем выдан", "ОВД Тверского района");

    assert!(item.matches("Паспорт"));
    assert!(item.matches("Тверского"), "видимые поля ищутся");
    assert!(
        !item.matches("4510123456"),
        "номер документа не индексируется"
    );
}
