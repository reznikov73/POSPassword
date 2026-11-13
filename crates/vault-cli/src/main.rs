//! Небольшой CLI поверх `vault-core`: нужен, чтобы проверять ядро
//! без графического интерфейса и уметь работать из скриптов.

use std::env;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use vault_core::{Error, GeneratorOptions, Item, ItemKind, Result, Vault, generate_password};

const HELP: &str = "\
Хранитель — локальное хранилище паролей

  vault-cli init <файл>                 создать новое хранилище
  vault-cli list <файл> [запрос]        список записей
  vault-cli show <файл> <запрос>        показать запись (пароль скрыт)
  vault-cli show <файл> <запрос> --reveal
  vault-cli add <файл> [--generate] [--length N]
  vault-cli passwd <файл>               сменить мастер-пароль
  vault-cli gen [--length N]            сгенерировать пароль
  vault-cli help

Мастер-пароль запрашивается интерактивно. Для скриптов можно задать
переменную окружения VAULT_PASSWORD (не используйте её на общих машинах).";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Ошибка: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(command) = args.first() else {
        println!("{HELP}");
        return Ok(());
    };

    let rest = &args[1..];
    match command.as_str() {
        "help" | "--help" | "-h" => {
            println!("{HELP}");
            Ok(())
        }
        "init" => init(rest),
        "list" => list(rest),
        "show" => show(rest),
        "add" => add(rest),
        "passwd" => passwd(rest),
        "gen" => generate_cmd(rest),
        other => {
            println!("Неизвестная команда: {other}\n\n{HELP}");
            Ok(())
        }
    }
}

fn init(args: &[String]) -> Result<()> {
    let path = require_path(args)?;
    let password = read_master_password("Новый мастер-пароль: ")?;
    let repeat = read_master_password("Повторите пароль: ")?;
    if password != repeat {
        return Err(Error::InvalidFormat("пароли не совпадают".into()));
    }

    let vault = Vault::create(&path, &password)?;
    println!(
        "Хранилище создано: {}",
        vault.path().display()
    );
    Ok(())
}

fn list(args: &[String]) -> Result<()> {
    let path = require_path(args)?;
    let query = args.get(1).map(String::as_str).unwrap_or("");
    let vault = open_vault(&path)?;

    let items = vault.search(query);
    if items.is_empty() {
        println!("Записей нет.");
        return Ok(());
    }

    println!("{:<32} {:<24} {}", "Название", "Логин", "Сайт");
    println!("{}", "-".repeat(80));
    for item in items {
        println!(
            "{:<32} {:<24} {}",
            truncate(&item.title, 32),
            truncate(&item.username, 24),
            item.url
        );
    }
    println!("\nВсего: {}", vault.items().len());
    Ok(())
}

fn show(args: &[String]) -> Result<()> {
    let path = require_path(args)?;
    let query = args
        .get(1)
        .ok_or_else(|| Error::InvalidFormat("укажите запрос для поиска".into()))?;
    let reveal = args.iter().any(|a| a == "--reveal");
    let vault = open_vault(&path)?;

    let item = vault
        .search(query)
        .into_iter()
        .next()
        .ok_or_else(|| Error::ItemNotFound(query.clone()))?;

    println!("Название: {}", item.title);
    println!("Тип:      {}", item.kind.label());
    if !item.username.is_empty() {
        println!("Логин:    {}", item.username);
    }
    if !item.password.is_empty() {
        if reveal {
            println!("Пароль:   {}", item.password);
        } else {
            println!("Пароль:   •••••••• (--reveal чтобы показать)");
        }
    }
    if !item.url.is_empty() {
        println!("Сайт:     {}", item.url);
    }
    if !item.notes.is_empty() {
        println!("Заметки:  {}", item.notes);
    }
    Ok(())
}

fn add(args: &[String]) -> Result<()> {
    let path = require_path(args)?;
    let generate = args.iter().any(|a| a == "--generate");
    let length = parse_length(args)?.unwrap_or(20);

    let mut vault = open_vault(&path)?;

    let title = prompt_line("Название: ")?;
    if title.trim().is_empty() {
        return Err(Error::InvalidFormat("название не может быть пустым".into()));
    }
    let username = prompt_line("Логин: ")?;
    let password = if generate {
        let generated = generate_password(&GeneratorOptions {
            length,
            ..GeneratorOptions::default()
        })?;
        println!("Сгенерированный пароль: {generated}");
        generated
    } else {
        read_secret_line("Пароль: ")?
    };
    let url = prompt_line("Сайт: ")?;
    let notes = prompt_line("Заметки: ")?;

    let kind = if username.is_empty() && password.is_empty() {
        ItemKind::SecureNote
    } else {
        ItemKind::Login
    };

    let mut item = Item::new(kind, title.trim());
    item.username = username.trim().to_string();
    item.password = password;
    item.url = url.trim().to_string();
    item.notes = notes;

    let id = vault.add(item)?;
    println!("Запись добавлена: {id}");
    Ok(())
}

fn passwd(args: &[String]) -> Result<()> {
    let path = require_path(args)?;
    let mut vault = open_vault(&path)?;
    let password = read_secret_line("Новый мастер-пароль: ")?;
    let repeat = read_secret_line("Повторите пароль: ")?;
    if password != repeat {
        return Err(Error::InvalidFormat("пароли не совпадают".into()));
    }
    vault.change_password(&password)?;
    println!("Мастер-пароль изменён.");
    Ok(())
}

fn generate_cmd(args: &[String]) -> Result<()> {
    let length = parse_length(args)?.unwrap_or(20);
    let password = generate_password(&GeneratorOptions {
        length,
        ..GeneratorOptions::default()
    })?;
    println!("{password}");
    Ok(())
}

fn open_vault(path: impl AsRef<std::path::Path>) -> Result<Vault> {
    let password = read_master_password("Мастер-пароль: ")?;
    Vault::open(path.as_ref().to_path_buf(), &password)
}

fn require_path(args: &[String]) -> Result<PathBuf> {
    args.first()
        .map(PathBuf::from)
        .ok_or_else(|| Error::InvalidFormat("укажите путь к файлу хранилища".into()))
}

fn parse_length(args: &[String]) -> Result<Option<usize>> {
    let Some(pos) = args.iter().position(|a| a == "--length") else {
        return Ok(None);
    };
    let value = args
        .get(pos + 1)
        .ok_or_else(|| Error::InvalidFormat("--length требует значения".into()))?;
    let parsed: usize = value
        .parse()
        .map_err(|_| Error::InvalidFormat(format!("некорректная длина: {value}")))?;
    if !(8..=128).contains(&parsed) {
        return Err(Error::InvalidFormat("длина должна быть от 8 до 128".into()));
    }
    Ok(Some(parsed))
}

fn read_master_password(prompt: &str) -> Result<String> {
    if let Ok(value) = env::var("VAULT_PASSWORD") {
        return Ok(value);
    }
    read_secret_line(prompt)
}

/// Читает секрет: в терминале — без эха, из пайпа — обычной строкой.
fn read_secret_line(prompt: &str) -> Result<String> {
    if io::stdin().is_terminal() {
        return rpassword::prompt_password_stderr(prompt).map_err(Error::Io);
    }
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

fn prompt_line(prompt: &str) -> Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

fn truncate(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_string();
    }
    let cut: String = value.chars().take(width.saturating_sub(1)).collect();
    format!("{cut}…")
}
