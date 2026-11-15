//! Разбор скопированного текста с логином и паролем.
//!
//! Помогает перенести пару «логин — пароль» из браузера, письма или чата
//! одним Ctrl+V вместо ручного заполнения полей. Понимает подписи
//! («Логин: …» / «Пароль: …»), строки вида «логин пароль», «login:password»,
//! «сайт логин пароль» и адреса с логином и паролем внутри:
//! `https://ivan:secret@mail.ru/`.

use crate::model::Item;

/// Что удалось понять из вставленного текста.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedCredentials {
    pub title: String,
    pub username: String,
    pub password: String,
    pub url: String,
}

impl ParsedCredentials {
    /// Есть хотя бы логин или пароль — из этого можно собрать запись.
    pub fn is_recognized(&self) -> bool {
        !self.username.is_empty() || !self.password.is_empty()
    }

    /// Собирает из разбора готовую запись-логин.
    pub fn into_item(self) -> Item {
        let ParsedCredentials {
            title,
            username,
            password,
            url,
        } = self;
        let title = if title.is_empty() {
            "Новая запись".to_string()
        } else {
            title
        };
        let mut item = Item::new(crate::model::ItemKind::Login, title);
        item.username = username;
        item.password = password;
        item.url = url;
        item
    }
}

/// Разбирает текст, вставленный пользователем, на логин, пароль и сайт.
pub fn parse_credentials(text: &str) -> ParsedCredentials {
    let mut out = ParsedCredentials::default();
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    // 1. Строки с подписями: «Логин: ivan», «Password: qwerty», «Сайт: …».
    let mut rest: Vec<&str> = Vec::new();
    for line in &lines {
        let mut recognized = false;
        if let Some((label, value)) = line.split_once(':') {
            let label = label.trim().to_lowercase();
            let value = value.trim();
            if !value.is_empty() {
                match label.as_str() {
                    "логин"
                    | "login"
                    | "username"
                    | "имя пользователя"
                    | "пользователь"
                    | "почта"
                    | "e-mail"
                    | "email" => {
                        out.username = value.to_string();
                        recognized = true;
                    }
                    "пароль" | "password" | "pass" | "пасс" => {
                        out.password = value.to_string();
                        recognized = true;
                    }
                    "сайт" | "site" | "url" | "адрес" | "адрес сайта" | "ссылка" | "link" =>
                    {
                        out.url = value.to_string();
                        recognized = true;
                    }
                    "название" | "имя" | "title" | "name" => {
                        out.title = value.to_string();
                        recognized = true;
                    }
                    _ => {}
                }
            }
        }
        if !recognized {
            rest.push(line);
        }
    }

    // 2. Токены из оставшихся строк.
    let mut tokens: Vec<&str> = rest
        .iter()
        .flat_map(|line| line.split_whitespace())
        .collect();

    // 2а. Адрес с логином и паролем внутри: https://ivan:secret@mail.ru/…
    if let Some(pos) = tokens
        .iter()
        .position(|tok| tok.contains("://") && token_has_credentials(tok))
    {
        let tok = tokens.remove(pos);
        let (scheme, after) = tok.split_once("://").unwrap_or(("", tok));
        if let Some((userinfo, host_part)) = after.split_once('@')
            && let Some((user, pass)) = userinfo.split_once(':')
        {
            if out.username.is_empty() {
                out.username = user.to_string();
            }
            if out.password.is_empty() {
                out.password = pass.to_string();
            }
            if out.url.is_empty() {
                out.url = format!("{scheme}://{host_part}");
            }
        }
    }

    // 2б. Первый адрес — сайт записи.
    if out.url.is_empty()
        && let Some(pos) = tokens.iter().position(|tok| {
            tok.starts_with("http://") || tok.starts_with("https://") || tok.starts_with("www.")
        })
    {
        out.url = tokens.remove(pos).to_string();
    }

    // 2в. «сайт логин пароль»: первый токен с точкой — адрес.
    if out.url.is_empty() && tokens.len() >= 3 && looks_like_domain(tokens[0]) {
        out.url = tokens.remove(0).to_string();
    }

    // 2г. Остальные токены — сначала логин, затем пароль. Если единственный
    // токен потом распадётся на пару «логин:пароль», логин заменим целиком.
    for token in &tokens {
        if out.username.is_empty() {
            out.username = (*token).to_string();
        } else if out.password.is_empty() {
            out.password = (*token).to_string();
        } else {
            break;
        }
    }

    // 2д. Один токен с разделителем «user:pass» или «user;pass». Логин,
    // набранный этим же токеном, заменяется парой; чужой (из подписи) — нет.
    if tokens.len() == 1 {
        let token = tokens[0];
        for sep in [':', ';'] {
            let Some((left, right)) = token.split_once(sep) else {
                continue;
            };
            if left.is_empty() || right.is_empty() || left.contains("://") || left.contains('@') {
                continue;
            }
            if out.username == token {
                out.username = left.to_string();
                out.password = right.to_string();
            } else if out.password.is_empty() {
                out.password = right.to_string();
            }
            break;
        }
    }

    // 3. Название: домен сайта, иначе логин.
    if out.title.is_empty() {
        if let Some(domain) = domain_of(&out.url) {
            out.title = domain;
        } else if !out.username.is_empty() {
            out.title = out.username.clone();
        }
    }

    out
}

/// `https://ivan:secret@mail.ru/…` — адрес с данными для входа внутри.
fn token_has_credentials(token: &str) -> bool {
    let Some((_, after_scheme)) = token.split_once("://") else {
        return false;
    };
    match after_scheme.split_once('@') {
        Some((userinfo, _)) => userinfo.contains(':') && !userinfo.contains('/'),
        None => false,
    }
}

/// Похоже на домен: есть точка, нет пробелов, собаки и слэшей.
fn looks_like_domain(token: &str) -> bool {
    token.contains('.')
        && !token.contains('@')
        && !token.contains('/')
        && !token.contains("://")
        && token
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphanumeric())
}

/// Домен из адреса: без схемы, «www.», пути и порта.
fn domain_of(url: &str) -> Option<String> {
    let without_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let host = without_scheme.split('/').next().unwrap_or(without_scheme);
    let host = host.split('@').next_back().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    let host = host.trim_start_matches("www.");
    let host = host.trim();
    (!host.is_empty()).then(|| host.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_with_credentials_is_split() {
        let parsed = parse_credentials("https://ivan:secret@mail.ru/inbox");
        assert_eq!(parsed.username, "ivan");
        assert_eq!(parsed.password, "secret");
        assert_eq!(parsed.url, "https://mail.ru/inbox");
        assert_eq!(parsed.title, "mail.ru");
    }

    #[test]
    fn labeled_lines_russian() {
        let parsed = parse_credentials("Логин: petr\nПароль: qwerty123");
        assert_eq!(parsed.username, "petr");
        assert_eq!(parsed.password, "qwerty123");
        assert!(parsed.is_recognized());
    }

    #[test]
    fn labeled_lines_english_with_url() {
        let parsed = parse_credentials("login: a@b.ru\npassword: pass\nurl: https://b.ru");
        assert_eq!(parsed.username, "a@b.ru");
        assert_eq!(parsed.password, "pass");
        assert_eq!(parsed.url, "https://b.ru");
        assert_eq!(parsed.title, "b.ru");
    }

    #[test]
    fn site_login_password_line() {
        let parsed = parse_credentials("mail.ru ivan@example.com Pa$$w0rd");
        assert_eq!(parsed.url, "mail.ru");
        assert_eq!(parsed.username, "ivan@example.com");
        assert_eq!(parsed.password, "Pa$$w0rd");
        assert_eq!(parsed.title, "mail.ru");
    }

    #[test]
    fn two_tokens_are_login_then_password() {
        let parsed = parse_credentials("user@mail.ru Pa$$w0rd");
        assert_eq!(parsed.username, "user@mail.ru");
        assert_eq!(parsed.password, "Pa$$w0rd");
        // Первый токен с точкой не съедается сайтом: токенов всего два.
        assert_eq!(parsed.title, "user@mail.ru");
        assert!(parsed.url.is_empty());
    }

    #[test]
    fn colon_pair_and_semicolon_pair() {
        let parsed = parse_credentials("user:pass");
        assert_eq!(parsed.username, "user");
        assert_eq!(parsed.password, "pass");

        let parsed = parse_credentials("user;pass");
        assert_eq!(parsed.username, "user");
        assert_eq!(parsed.password, "pass");
    }

    #[test]
    fn email_only_is_recognized() {
        let parsed = parse_credentials("  ivan@mail.ru  ");
        assert_eq!(parsed.username, "ivan@mail.ru");
        assert!(parsed.password.is_empty());
        assert!(parsed.is_recognized());
    }

    #[test]
    fn empty_text_is_not_recognized() {
        assert!(!parse_credentials("").is_recognized());
        assert!(!parse_credentials("   \n  ").is_recognized());
    }

    #[test]
    fn arbitrary_text_takes_first_two_tokens() {
        // Разбор намеренно простительный: предпросмотр в окне покажет,
        // что получилось, и пользователь поправит поля перед созданием.
        let parsed = parse_credentials("просто текст без данных");
        assert_eq!(parsed.username, "просто");
        assert_eq!(parsed.password, "текст");
    }

    #[test]
    fn into_item_fills_fields() {
        let parsed = parse_credentials("https://ivan:secret@mail.ru/inbox");
        let item = parsed.into_item();
        assert_eq!(item.kind, crate::model::ItemKind::Login);
        assert_eq!(item.title, "mail.ru");
        assert_eq!(item.username, "ivan");
        assert_eq!(item.password, "secret");
        assert_eq!(item.url, "https://mail.ru/inbox");
    }
}
