use vault_core::Totp;

/// Секрет из RFC 6238, приложение B: ASCII «12345678901234567890».
const SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

#[test]
fn default_is_six_digits_thirty_seconds() {
    let totp = Totp::parse(SECRET).expect("секрет");
    assert_eq!(totp.digits(), 6);
    assert_eq!(totp.period(), 30);
    // RFC 6238 при T=59 даёт 94287082; при шести цифрах — последние шесть.
    assert_eq!(totp.code_at(59), "287082");
}

#[test]
fn otpauth_uri_sets_parameters() {
    let eight = format!("otpauth://totp/Hranitel:user?secret={SECRET}&digits=8");
    let totp = Totp::parse(&eight).expect("ссылка");
    assert_eq!(totp.digits(), 8);
    assert_eq!(totp.period(), 30);
    assert_eq!(totp.code_at(59), "94287082");

    // Период из ссылки меняет счётчик: при 60 секундах T=59 — это ещё первый интервал.
    let slow = format!("otpauth://totp/Hranitel:user?secret={SECRET}&period=60");
    let totp = Totp::parse(&slow).expect("ссылка с периодом");
    assert_eq!(totp.period(), 60);
    assert_eq!(totp.code_at(59), totp.code_at(0));
}

#[test]
fn base32_with_spaces_and_lowercase() {
    let spaced = "gezd gnbv gy3t qojq gezd gnbv gy3t qojq";
    let totp = Totp::parse(spaced).expect("секрет");
    assert_eq!(totp.code_at(59), "287082");
}

#[test]
fn invalid_secret_is_rejected() {
    assert!(Totp::parse("не-base32!").is_err());
    assert!(Totp::parse("   ").is_err());
    assert!(Totp::parse("otpauth://totp/x?digits=6").is_err());
}

#[test]
fn seconds_left_counts_down() {
    let totp = Totp::parse(SECRET).expect("секрет");
    assert_eq!(totp.seconds_left(0), 30);
    assert_eq!(totp.seconds_left(59), 1);
    assert_eq!(totp.seconds_left(60), 30);
}

#[test]
fn debug_does_not_leak_secret() {
    let totp = Totp::parse(SECRET).expect("секрет");
    let text = format!("{totp:?}");
    assert!(!text.contains("GEZD"));
}
