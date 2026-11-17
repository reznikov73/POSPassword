//! Встраивает иконку и сведения о версии в `vault-win.exe`.
//!
//! Без этого Windows рисует стандартную заглушку: в проводнике, на панели
//! задач и в области уведомлений. Трей берёт иконку из exe через
//! `ExtractIconW`, поэтому одна и та же картинка работает везде.

fn main() {
    println!("cargo:rerun-if-changed=icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("icon.ico");
    if let Err(err) = resource.compile() {
        panic!("не удалось встроить иконку в exe: {err}");
    }
}
