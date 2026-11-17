use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use uuid::Uuid;
use zeroize::Zeroizing;

use crate::audit::{AuditReport, audit};
use crate::crypto::{self, KEY_LEN, KdfParams, NONCE_LEN, SALT_LEN};
use crate::describe;
use crate::export::{ExportFormat, export_csv, export_json};
use crate::hello::{self, HelloEnvelope};
use crate::lock;
use crate::model::{Item, VaultData, now_secs};
use crate::{Error, Result};

pub const MAGIC: &[u8; 8] = b"OPVAULT1";
pub const FORMAT_VERSION: u16 = 1;

const KDF_ARGON2ID: u8 = 1;
const HEADER_LEN: usize = 8 + 2 + 1 + 4 * 3 + SALT_LEN + NONCE_LEN;
const MIN_PASSWORD_LEN: usize = 8;

/// Пресет стойкости Argon2 при создании и смене мастер-пароля (Б-6).
///
/// Параметры пишутся в заголовок файла под AAD, поэтому усиленный файл
/// открывается только с теми же параметрами — формат не меняется.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KdfPreset {
    /// 64 МиБ, 3 итерации — как в предыдущих версиях.
    Standard,
    /// 128 МиБ, 4 итерации.
    Hardened,
    /// 256 МиБ, 4 итерации; на слабых ПК разблокировка занимает секунды.
    Maximum,
}

impl KdfPreset {
    pub fn params(self) -> KdfParams {
        match self {
            Self::Standard => KdfParams::default(),
            Self::Hardened => KdfParams {
                m_cost: 128 * 1024,
                t_cost: 4,
                p_cost: 1,
            },
            Self::Maximum => KdfParams {
                m_cost: 256 * 1024,
                t_cost: 4,
                p_cost: 1,
            },
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "Стандарт (64 МиБ, 3 итерации)",
            Self::Hardened => "Усиленный (128 МиБ, 4 итерации)",
            Self::Maximum => "Максимальный (256 МиБ, 4 итерации)",
        }
    }
}

/// Заголовок файла. Хранится в открытом виде, но целиком аутентифицируется
/// как AAD, поэтому изменить параметры KDF или соль незаметно нельзя.
struct Header {
    version: u16,
    kdf: KdfParams,
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
}

impl Header {
    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.push(KDF_ARGON2ID);
        out.extend_from_slice(&self.kdf.m_cost.to_le_bytes());
        out.extend_from_slice(&self.kdf.t_cost.to_le_bytes());
        out.extend_from_slice(&self.kdf.p_cost.to_le_bytes());
        out.extend_from_slice(&self.salt);
        out.extend_from_slice(&self.nonce);
        out
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_LEN {
            return Err(Error::InvalidFormat("файл короче заголовка".into()));
        }
        if &bytes[0..8] != MAGIC {
            return Err(Error::InvalidFormat("неверная сигнатура".into()));
        }
        let version = u16::from_le_bytes([bytes[8], bytes[9]]);
        if version != FORMAT_VERSION {
            return Err(Error::UnsupportedVersion(version));
        }
        let kdf_id = bytes[10];
        if kdf_id != KDF_ARGON2ID {
            return Err(Error::InvalidFormat(format!(
                "неизвестный алгоритм вывода ключа: {kdf_id}"
            )));
        }
        let m_cost = u32::from_le_bytes([bytes[11], bytes[12], bytes[13], bytes[14]]);
        let t_cost = u32::from_le_bytes([bytes[15], bytes[16], bytes[17], bytes[18]]);
        let p_cost = u32::from_le_bytes([bytes[19], bytes[20], bytes[21], bytes[22]]);

        let mut salt = [0u8; SALT_LEN];
        salt.copy_from_slice(&bytes[23..23 + SALT_LEN]);
        let nonce_start = 23 + SALT_LEN;
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&bytes[nonce_start..nonce_start + NONCE_LEN]);

        let kdf = KdfParams {
            m_cost,
            t_cost,
            p_cost,
        };
        kdf.validate()?;

        Ok(Self {
            version,
            kdf,
            salt,
            nonce,
        })
    }
}

/// Открытое (разблокированное) хранилище.
///
/// Ключ живёт в памяти и зануляется при уничтожении структуры. Каждая
/// изменяющая операция сразу пишет файл, поэтому отдельного «сохранить» нет.
pub struct Vault {
    path: PathBuf,
    key: Zeroizing<[u8; KEY_LEN]>,
    kdf: KdfParams,
    salt: [u8; SALT_LEN],
    data: VaultData,
    /// Растёт при каждом изменении данных; интерфейс использует, чтобы
    /// не пересчитывать аудит каждый кадр.
    revision: u64,
    /// Блокировка от параллельной записи (Ф-6): живёт, пока открыто
    /// хранилище; в `save` писать можно, конкурентов быть не должно.
    lock: lock::FileLock,
}

impl Vault {
    /// Создаёт новое пустое хранилище со стандартным пресетом Argon2.
    /// Существующий файл не перезаписывается.
    pub fn create(path: impl Into<PathBuf>, password: &str) -> Result<Self> {
        Self::create_with_preset(path, password, KdfPreset::Standard)
    }

    /// Создаёт новое пустое хранилище с выбранным пресетом стойкости (Б-6).
    pub fn create_with_preset(
        path: impl Into<PathBuf>,
        password: &str,
        preset: KdfPreset,
    ) -> Result<Self> {
        let path = path.into();
        if path.exists() {
            return Err(Error::InvalidFormat(format!(
                "файл уже существует: {}",
                path.display()
            )));
        }
        validate_master_password(password)?;
        let lock = lock::FileLock::acquire(&path)?;

        let kdf = preset.params();
        let mut salt = [0u8; SALT_LEN];
        crypto::random_bytes(&mut salt);
        let key = crypto::derive_key(password.as_bytes(), &salt, kdf)?;

        let vault = Self {
            path,
            key,
            kdf,
            salt,
            data: VaultData::default(),
            revision: 0,
            lock,
        };
        vault.save()?;
        // У пути мог остаться сайдкар от прежнего хранилища: он заворачивал
        // другой ключ и больше не действителен.
        let _ = remove_hello_sidecar(vault.path());
        Ok(vault)
    }

    /// Открывает и расшифровывает хранилище. Захватывает блокировку файла
    /// до закрытия хранилища: вторая программа получит «занято» (Ф-6).
    pub fn open(path: impl Into<PathBuf>, password: &str) -> Result<Self> {
        let path = path.into();
        let lock = lock::FileLock::acquire(&path)?;
        let bytes = fs::read(&path)?;
        let header = Header::decode(&bytes)?;

        let key = crypto::derive_key(password.as_bytes(), &header.salt, header.kdf)?;
        Self::open_decrypted(path, key, header, &bytes, lock)
    }

    /// Открывает хранилище ключом, развёрнутым из обёртки Windows Hello.
    ///
    /// `wrapping_key` — ключ обёртки, снятый приложением с DPAPI-защиты после
    /// подтверждения присутствия пользователя (Windows Hello). Отсутствие
    /// или повреждение сайдкара — ошибка: приложение откатывается на ввод
    /// мастер-пароля.
    pub fn open_with_hello(path: impl Into<PathBuf>, wrapping_key: &[u8; KEY_LEN]) -> Result<Self> {
        let path = path.into();
        let envelope = HelloEnvelope::read(&hello::sidecar_path(&path))?;
        let key = envelope.open(wrapping_key)?;

        let lock = lock::FileLock::acquire(&path)?;
        let bytes = fs::read(&path)?;
        let header = Header::decode(&bytes)?;
        Self::open_decrypted(path, key, header, &bytes, lock)
    }

    /// Общая часть `open` и `open_with_hello`: расшифровка и разбор данных.
    fn open_decrypted(
        path: PathBuf,
        key: Zeroizing<[u8; KEY_LEN]>,
        header: Header,
        bytes: &[u8],
        lock: lock::FileLock,
    ) -> Result<Self> {
        let header_bytes = &bytes[..HEADER_LEN];
        let plaintext = crypto::open(&key, &header.nonce, &bytes[HEADER_LEN..], header_bytes)?;
        let data: VaultData = serde_json::from_slice(&plaintext)?;

        Ok(Self {
            path,
            key,
            kdf: header.kdf,
            salt: header.salt,
            data,
            revision: 0,
            lock,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn items(&self) -> &[Item] {
        &self.data.items
    }

    /// Элементы в алфавитном порядке без учёта регистра.
    pub fn items_sorted(&self) -> Vec<&Item> {
        let mut items: Vec<&Item> = self.data.items.iter().collect();
        items.sort_by_key(|i| i.title.to_lowercase());
        items
    }

    pub fn get(&self, id: Uuid) -> Option<&Item> {
        self.data.items.iter().find(|i| i.id == id)
    }

    pub fn search(&self, query: &str) -> Vec<&Item> {
        let mut found: Vec<&Item> = self
            .data
            .items
            .iter()
            .filter(|i| i.matches(query))
            .collect();
        found.sort_by_key(|i| i.title.to_lowercase());
        found
    }

    pub fn add(&mut self, mut item: Item) -> Result<Uuid> {
        item.created_at = now_secs();
        item.updated_at = item.created_at;
        let id = item.id;
        self.data.items.push(item);
        self.revision += 1;
        self.save()?;
        Ok(id)
    }

    /// Добавляет несколько записей и сохраняет файл один раз.
    /// Нужно для импорта: сотни записей — одна запись на диск.
    pub fn add_many(&mut self, items: Vec<Item>) -> Result<usize> {
        if items.is_empty() {
            return Ok(0);
        }
        let count = items.len();
        let now = now_secs();
        for mut item in items {
            item.created_at = now;
            item.updated_at = now;
            self.data.items.push(item);
        }
        self.revision += 1;
        self.save()?;
        Ok(count)
    }

    /// Заменяет запись с тем же id, сохраняя дату создания. Если пароль
    /// изменился, прежний непустой уходит в историю пароля (Ф-2) — так
    /// правка из окна, из моста браузера и из CLI пишется одинаково.
    pub fn update(&mut self, mut item: Item) -> Result<()> {
        let existing = self
            .data
            .items
            .iter_mut()
            .find(|i| i.id == item.id)
            .ok_or_else(|| Error::ItemNotFound(item.id.to_string()))?;
        item.created_at = existing.created_at;
        item.updated_at = now_secs();
        let old_history = std::mem::take(&mut existing.password_history);
        let old_password = existing.password.clone();
        let old_updated = existing.updated_at;
        item.absorb_password_history(old_history, &old_password, old_updated);
        *existing = item;
        self.revision += 1;
        self.save()
    }

    /// Возвращает версию из истории паролей текущей записью (И-5):
    /// выбранная версия становится паролем, прежний текущий уходит в
    /// историю. Отдельный метод, а не `update`: там история собирается
    /// по сравнению паролей, а здесь она уже перестроена.
    pub fn restore_password_version(&mut self, id: Uuid, index: usize) -> Result<()> {
        let item = self
            .data
            .items
            .iter_mut()
            .find(|i| i.id == id)
            .ok_or_else(|| Error::ItemNotFound(id.to_string()))?;
        item.restore_password_version(index)?;
        item.touch();
        self.revision += 1;
        self.save()
    }

    /// Проверяет мастер-пароль открытого хранилища без обращения к файлу
    /// (сверкой выведенного ключа). Нужен диалогу смены пароля: повторное
    /// `open` теперь упирается в собственную блокировку (Ф-6).
    pub fn verify_password(&self, password: &str) -> bool {
        match crypto::derive_key(password.as_bytes(), &self.salt, self.kdf) {
            Ok(key) => key[..] == self.key[..],
            Err(_) => false,
        }
    }

    /// Замер времени вывода ключа на этом компьютере (Б-6): подсказка
    /// рядом с выбором пресета. Пароль и соль — случайные, результат
    /// никуда не идёт.
    pub fn measure_kdf(params: KdfParams) -> std::time::Duration {
        let salt = [7u8; SALT_LEN];
        let started = std::time::Instant::now();
        let _ = crypto::derive_key(b"measure", &salt, params);
        started.elapsed()
    }

    /// Возвращает удалённую запись без изменений id и дат (для «Вернуть»).
    /// В отличие от `add`, дата создания не перезаписывается.
    pub fn restore(&mut self, item: Item) -> Result<()> {
        if self.data.items.iter().any(|i| i.id == item.id) {
            return Err(Error::InvalidFormat(format!(
                "запись {} уже существует",
                item.id
            )));
        }
        self.data.items.push(item);
        self.revision += 1;
        self.save()
    }

    /// Номер ревизии данных: растёт при каждом изменении.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn remove(&mut self, id: Uuid) -> Result<()> {
        let before = self.data.items.len();
        self.data.items.retain(|i| i.id != id);
        if self.data.items.len() == before {
            return Err(Error::ItemNotFound(id.to_string()));
        }
        self.revision += 1;
        self.save()
    }

    /// Заполняет пустые заметки осмысленными описаниями и сохраняет файл
    /// один раз. Возвращает число описанных записей. Существующие заметки
    /// пользователя не трогаются.
    pub fn describe_missing_notes(&mut self) -> Result<usize> {
        let mut count = 0;
        for item in &mut self.data.items {
            if item.notes.trim().is_empty() {
                let description = describe::auto_description(item);
                item.notes = description;
                item.touch();
                count += 1;
            }
        }
        if count > 0 {
            self.revision += 1;
            self.save()?;
        }
        Ok(count)
    }

    /// Меняет мастер-пароль с теми же параметрами Argon2.
    pub fn change_password(&mut self, new_password: &str) -> Result<()> {
        self.change_password_with_preset(new_password, self.kdf)
    }

    /// Меняет мастер-пароль и параметры Argon2 (апгрейд пресета, Б-6):
    /// новая соль, новый ключ, та же полезная нагрузка. Обёртка Windows
    /// Hello заворачивала прежний ключ — сайдкар удаляется.
    pub fn change_password_with_preset(
        &mut self,
        new_password: &str,
        kdf: KdfParams,
    ) -> Result<()> {
        validate_master_password(new_password)?;
        let mut salt = [0u8; SALT_LEN];
        crypto::random_bytes(&mut salt);
        self.key = crypto::derive_key(new_password.as_bytes(), &salt, kdf)?;
        self.kdf = kdf;
        self.salt = salt;
        self.save()?;
        remove_hello_sidecar(&self.path)
    }

    /// Текущие параметры Argon2 (для подсказки в интерфейсе).
    pub fn kdf_params(&self) -> KdfParams {
        self.kdf
    }

    /// Удерживается ли блокировка файла (всегда true у открытого хранилища;
    /// метод нужен, чтобы поле не казалось мёртвым, и для тестов).
    pub fn holds_file_lock(&self) -> bool {
        self.lock.holds()
    }

    /// Включает разблокировку через Windows Hello: заворачивает ключ хранилища
    /// и пишет сайдкар рядом с файлом.
    ///
    /// `wrapping_key` — свежий случайный ключ обёртки (`generate_wrapping_key`);
    /// `dpapi_blob` — этот же ключ, защищённый приложением через DPAPI.
    /// Ядро трактует блоб как непрозрачные данные.
    pub fn enable_hello(&mut self, wrapping_key: &[u8; KEY_LEN], dpapi_blob: &[u8]) -> Result<()> {
        let envelope = HelloEnvelope::seal(&self.key, wrapping_key, dpapi_blob.to_vec());
        envelope.write(&hello::sidecar_path(&self.path))
    }

    /// Выключает разблокировку через Windows Hello: удаляет сайдкар.
    /// Отсутствие сайдкара не ошибка.
    pub fn disable_hello(&mut self) -> Result<()> {
        remove_hello_sidecar(&self.path)
    }

    /// Аудит записей: пароли, сроки действия, давно не обновлённые.
    pub fn audit(&self) -> AuditReport {
        audit(&self.data.items, now_secs())
    }

    /// Экспорт всех записей в выбранный формат.
    pub fn export(&self, format: ExportFormat) -> Result<String> {
        match format {
            ExportFormat::Json => export_json(&self.data.items, now_secs()),
            ExportFormat::Csv => export_csv(&self.data.items),
        }
    }

    /// Перешифровывает и записывает файл. Nonce всегда новый.
    pub fn save(&self) -> Result<()> {
        let mut nonce = [0u8; NONCE_LEN];
        crypto::random_bytes(&mut nonce);

        let header = Header {
            version: FORMAT_VERSION,
            kdf: self.kdf,
            salt: self.salt,
            nonce,
        };
        let header_bytes = header.encode();

        let plaintext = Zeroizing::new(serde_json::to_vec(&self.data)?);
        let ciphertext = crypto::seal(&self.key, &nonce, &plaintext, &header_bytes)?;

        let mut out = Vec::with_capacity(header_bytes.len() + ciphertext.len());
        out.extend_from_slice(&header_bytes);
        out.extend_from_slice(&ciphertext);

        // Резервная копия: прежняя версия остаётся рядом как <файл>.bak (Ф-5).
        // Копирование до переименования — не бывает момента, когда основного
        // файла нет. Ошибка копии не должна мешать сохранить основную версию.
        if self.path.exists() {
            let _ = fs::copy(&self.path, backup_path(&self.path));
        }
        write_atomic(&self.path, &out)
    }
}

/// Путь автоматической резервной копии: `<файл-хранилища>.bak`.
pub fn backup_path(vault_path: &Path) -> PathBuf {
    let mut name = vault_path.as_os_str().to_os_string();
    name.push(".bak");
    PathBuf::from(name)
}

impl std::fmt::Debug for Vault {
    // Ключ и содержимое в отладочный вывод не попадают.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("path", &self.path)
            .field("items", &self.data.items.len())
            .finish_non_exhaustive()
    }
}

fn validate_master_password(password: &str) -> Result<()> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(Error::InvalidFormat(format!(
            "мастер-пароль должен быть не короче {MIN_PASSWORD_LEN} символов"
        )));
    }
    Ok(())
}

/// Пишет файл через временный файл и переименование, чтобы обрыв записи
/// не оставил наполовину записанное хранилище.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    fs::create_dir_all(&dir)?;

    let mut tmp = tempfile::NamedTempFile::new_in(&dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| Error::Io(e.error))?;
    Ok(())
}

/// Удаляет сайдкар Windows Hello, если он существует.
fn remove_hello_sidecar(vault_path: &Path) -> Result<()> {
    match fs::remove_file(hello::sidecar_path(vault_path)) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}
