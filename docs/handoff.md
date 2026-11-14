# Передача проекта «Хранитель» (handoff)

Документ для агента, который продолжит работу. Всё, что нужно знать, чтобы
собрать, проверить и не наступить на уже пройденные грабли.

Дата: 14 ноября 2026. Версия: **0.3.0**.

## 1. Что это

Локальный менеджер паролей для Windows. Один зашифрованный файл, без сети и аккаунтов.
Ядро на Rust, CLI и оконный клиент на egui/eframe.

Расположение: `E:\Work\VSC\POSPassword`
Репозиторий: https://github.com/reznikov73/POSPassword (приватный, ветка `main`)
Последний коммит на момент передачи: `090a8f4`

## 2. Быстрый старт

```powershell
cd E:\Work\VSC\POSPassword
cargo test --workspace                       # 42 теста, все должны пройти
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p vault-win
.\target\release\vault-win.exe
```

В VS Code: `Ctrl+Shift+B` — сборка, `F5` — отладка, `Ctrl+Shift+P` → «Задачи: Выполнить задачу» —
список задач на русском. Расширения уже установлены.

## 3. Архитектура

```
crates/vault-core   формат файла, криптография, модель, CRUD, поиск, генератор, импорт,
                    обёртка ключа для Windows Hello
crates/vault-cli    CLI: init, add, list, show, passwd, gen, import
apps/vault-win      egui-клиент: вход, список, карточка, импорт, темы, автоблокировка, буфер;
                    hello_win.rs — DPAPI и UserConsentVerifier
docs/               архитектура, категории данных, инструкции, этот файл
```

### Формат файла (v1, менять только с повышением версии)

```
magic "OPVAULT1"(8) | version u16 | kdf_id u8=1 | m_cost u32 | t_cost u32 | p_cost u32
| salt(16) | nonce(24) | ciphertext
```

- Ключ: `Argon2id(password, salt, m=65536 КиБ, t=3, p=1) -> 32 байта`.
- Шифр: XChaCha20-Poly1305, nonce новый при каждой записи.
- AAD: весь заголовок (0..63), поэтому параметры KDF и соль защищены от подмены.
- Полезная нагрузка: JSON `VaultData { schema: 3, items: [...] }`.
- Запись: временный файл в том же каталоге → `sync_all` → атомарное переименование.

### Сайдкар Windows Hello (v1, рядом с хранилищем)

```
magic "OPHELLO1"(8) | version u16=1 | nonce(12) | wrapped_key(48) | dpapi_blob(..)
```

- `wrapped_key` = AES-256-GCM(ключ обёртки, ключ хранилища); AAD — магия, версия, nonce.
- Ключ обёртки в открытом виде не хранится: `dpapi_blob` — он же под DPAPI (область
  пользователя), ядро его не интерпретирует.
- Путь: `<путь-хранилища>.hello`. В git не попадает (`*.hello` в `.gitignore`).
- Смена мастер-пароля и `Vault::create` по старому пути удаляют сайдкар.

### Модель

```rust
ItemKind { Login, SecureNote, Card, BankAccount, Passport, ForeignPassport, Snils,
           Inn, DriverLicense, MedicalPolicy, DigitalSignature, ApiKey, Wifi, Pin,
           Insurance, Property, Health, Vehicle }   // 18 типов, serde snake_case

Item { id: Uuid, kind, title, username, password, url, notes,
       tags: Vec<String>, fields: Vec<Field>, created_at, updated_at }

Field { label: String, value: String, sensitive: bool }
```

- `ItemKind::template()` — готовые поля для типа.
- `Item::with_template(kind, title)` — запись с шаблоном.
- `sensitive` поля маскируются, не индексируются поиском и зануляются в `Drop`.
- Пароль и заметки тоже зануляются в `Drop` у `Item`.

## 4. Что уже работает

- Создание и открытие хранилища, смена мастер-пароля.
- Логины и защищённые заметки, поиск по видимым полям.
- Генератор паролей (длина, классы, без `Il1O0o`).
- Карты и банковские счета; 14 шаблонов документов и ключей (всего 18 типов).
- Импорт: Bitwarden JSON, Bitwarden CSV, 1Password CSV, обычный CSV; дубликаты пропускаются.
- Буфер обмена с автоочисткой через 30 секунд.
- Автоблокировка через 5 минут бездействия.
- Светлая и тёмная тема, выбор сохраняется в `%APPDATA%\Hranitel\settings.txt`.
- Вход через Windows Hello: обёртка ключа + DPAPI + подтверждение присутствия;
  кнопки «Войти через Windows Hello» и «Hello: включён/выключен».
- CLI со всеми операциями.
- 42 теста: ядро (15), обёртка Hello (10), импорт (9), шаблоны (6), DPAPI (2).

## 5. Проверочный чек-лист

1. `cargo test --workspace` → 42 passed, 0 failed.
2. `cargo clippy --workspace --all-targets -- -D warnings` → без предупреждений.
3. Запустить `vault-win.exe` → светлая тема, экран входа, кнопка «Тема: светлая».
4. Создать хранилище (пароль от 8 символов) → основной экран, «Записей: 0».
5. «+» → тип «Паспорт РФ» → пять полей на месте, серия и номер скрыты.
6. Сохранить → найти поиском по названию; поиск по номеру паспорта ничего не даёт.
7. «Копировать» пароль → подождать 30 секунд → вставить: буфер пуст.
8. «Импорт…» → Bitwarden JSON → «Добавлено: N»; повторить → «Пропущено дубликатов: N».
9. Переключить тему → перезапустить → тема сохранилась.
10. Оставить окно на 5 минут → блокировка.
11. Открыть хранилище → правый верхний угол → «Hello: выключен» → кнопка стала
    «Hello: включён»; рядом с файлом появился `<файл>.hello`.
12. «Заблокировать» → на экране входа появилась кнопка «Войти через Windows Hello» →
    подтвердить Hello → хранилище открыто, мастер-пароль не спрашивался.
13. Отменить запрос Hello (Esc) → появилось сообщение с предложением ввести пароль;
    вход по паролю работает.
14. Сменить мастер-пароль через CLI → сайдкар исчез, вход через Hello недоступен,
    пока опция не включена заново.

## 6. Грабли (проверено на практике)

**egui/eframe 0.37**
- `App::update` больше нет: реализуется `fn ui(&mut self, ui: &mut egui::Ui, frame: &mut Frame)`.
- `TopBottomPanel`/`SidePanel` заменены на `egui::Panel::bottom(...)`; `CentralPanel::show(ui, ...)`.
- Вложенные панели внутри `Ui` легко получают нулевую высоту — в этом проекте вместо них
  фиксированная область контента + строка статуса.
- `clear_color` надо переопределять, иначе eframe заливает окно чёрным и светлая тема «не работает».
- Два `ScrollArea` без `id_salt` дают предупреждение о дублирующемся ID.

**Крейты**
- `argon2 0.6`: `Params::new(m, t, p, Some(32))`, `hash_password_into`.
- `chacha20poly1305 0.11`: `Aead::encrypt(&nonce, Payload { msg, aad })`.
- `rand 0.10`: `rand::fill`, `rand::rng()`, трейт `RngExt`.
- `rpassword 1.0`: функции `prompt_password_stderr`, старой `prompt_password` нет.
- edition 2024: `gen` — зарезервированное слово, функцию пришлось назвать `generate_cmd`.

**Окружение**
- В оболочке AutoClaw переменные `APPDATA` и `LOCALAPPDATA` пустые. Приложение и скрипты
  должны собирать путь от `USERPROFILE`. В обычной сессии Windows всё работает штатно.
- GitHub CLI установлен портативно: `%USERPROFILE%\AppData\Local\GitHubCLI\bin`, добавлен в PATH.
- `git push` работает через Windows Credential Manager (`credential.helper=manager`).
- Токены, которые передавались в чате, считаются скомпрометированными — их нужно отозвать
  в GitHub → Settings → Developer settings → Personal access tokens.
- При сборке в фоне проверяйте mtime бинарника: фильтрация вывода иногда скрывает ошибки.

**Windows Hello / DPAPI**
- `windows 0.62`: для DPAPI берём классический Win32 `CryptProtectData`/`CryptUnprotectData`
  (`Win32_Security_Cryptography`), а не WinRT `DataProtectionProvider`: синхронно, без квартиры.
- `UserConsentVerifier::RequestVerificationAsync` — WinRT, возвращает `IAsyncOperation`;
  блокирующее ожидание — `.join()` (крейт `windows-future`): ждёт событие и не прокачивает
  сообщения, поэтому вызывать только из фонового потока, не из UI-потока egui.
- На рабочем потоке перед WinRT нужен `CoInitializeEx(None, COINIT_MULTITHREADED)`
  (`Win32_System_Com`); результат можно игнорировать.
- DPAPI-блоб привязан к учётной записи и машине: копия сайдкара на другом ПК не
  разворачивается — это ожидаемо, а не баг.
- `aes-gcm 0.11` (aead 0.6): nonce — `Nonce<U12>` из `aes_gcm::aead::consts::U12`,
  `encrypt`/`decrypt` принимают `&Nonce`, ключ — `new_from_slice`.
- `cargo test -p vault-win` действительно вызывает DPAPI и проверяет раунд-трип —
  это единственный тест системных вызовов без GUI.

**Не коммитить**: `target`, `dist`, `*.opvault`, `*.vault`, `*.hello`, папку `NVIDIA Corporation`.

## 7. Ограничения текущей версии

- Нет синхронизации между устройствами и мобильного клиента.
- Нет автозаполнения в браузере и приложениях, нет passkeys.
- Windows Hello: только подтверждение присутствия, без TPM-привязки; доступность
  Hello проверяется при первом входе, а не при включении опции.
- TOTP только хранится как скрытое поле, коды не генерируются.
- 1Password 1PUX (архив) не читается, только CSV.
- Карты из Bitwarden CSV приходят без реквизитов (в CSV их нет).
- Нет напоминаний об истечении срока документов и карт.
- Внешнего аудита не было.

## 8. Следующий шаг

### Этап 1. Windows Hello — сделано в 0.3.0

Реализовано по плану предыдущей передачи проекта: ключ хранилища обёрнут AES-256-GCM случайным
ключом обёртки, ключ обёртки — под DPAPI (область пользователя), сайдкар
`<файл>.hello` рядом с хранилищем; на входе — `UserConsentVerifier` в фоновом
потоке. Файлы: `crates/vault-core/src/hello.rs`, `apps/vault-win/src/hello_win.rs`,
UI — `main.rs`. Тесты: 10 на обёртку и 2 на DPAPI.

Что осталось по теме (не блокирует Этап 2):
- проверять доступность Hello (`CheckAvailabilityAsync`) при включении опции;
- TPM-привязка (сейчас только DPAPI user-scope);
- ручной прогон пунктов 11–14 чек-листа на машине с настроенным Hello.

Критерии приёмки (выполнены):
- Без включённой опции поведение не меняется.
- С включённой: запуск → Hello → хранилище открыто, мастер-пароль не спрашивается.
- Отказ или ошибка Hello → предложение ввести мастер-пароль.
- Смена пароля инвалидирует обёртку (сайдкар удаляется).
- Тесты на wrap/unwrap и на отказ при повреждённой обёртке.

### Этап 2. Android-клиент (5–8 недель)

- Установить JDK 17 и Android SDK/NDK (в текущем окружении их нет).
- Обёртка `vault-core` через UniFFI, проект Kotlin + Jetpack Compose.
- `AutofillService`, `BiometricPrompt`, Keystore; позже — Credential Provider для passkeys.
- Критерии: открывает тот же файл, автозаполнение в Chrome и приложении, биометрия.

### Дальше

Синхронизация (E2E, сервер видит только шифротекст), браузерное расширение,
passkeys, внешний аудит. Порядок и оценки — в `docs/architecture.md`, раздел 7.

## 9. Правила проекта

- Интерфейс и документация — по-русски, код и идентификаторы — по-английски.
- Перед коммитом: `cargo fmt --all`, `cargo clippy -- -D warnings`, `cargo test --workspace`.
- Формат файла не менять без повышения `FORMAT_VERSION` и теста на старый файл.
- `vault-core` остаётся платформенно-независимым: никакого WinRT/DPAPI в ядре —
  эти вызовы только в `apps/vault-win` (ядро заберёт Android-клиент).
- Секреты не логировать, не печатать, не коммитить.
- Осмысленные сообщения коммитов по-русски.

## 10. Полезные команды

```powershell
cargo run -p vault-cli -- init  .\vault.opvault
cargo run -p vault-cli -- list  .\vault.opvault
cargo run -p vault-cli -- show  .\vault.opvault github --reveal
cargo run -p vault-cli -- import .\vault.opvault .\bitwarden_export.json
cargo run -p vault-cli -- gen   --length 24
cargo run -p vault-cli -- passwd .\vault.opvault
```