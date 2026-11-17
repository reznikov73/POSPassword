//! Мост к браузерному расширению: запросы, диалоги, сохранение.

use crate::*;

impl App {
    /// Разбирает запросы расширения. Один запрос — один ответ; если нужно
    /// решение человека, диалог забирает канал ответа себе.
    pub(crate) fn poll_browser_jobs(&mut self, ctx: &egui::Context) {
        while let Ok(job) = self.browser_jobs.try_recv() {
            self.handle_browser(job.request, job.reply);
            // Хранилище закрыто — поднимаем окно и ждём разблокировки,
            // запрос остаётся висеть в мосте (у него две минуты).
            if self.pending_browser.is_some() {
                self.show_main(ctx);
                break;
            }
            // Диалог открыт — за раз решаем один запрос, остальные подождут.
            if self.browser_ask.is_some() {
                break;
            }
        }
    }

    /// Обрабатывает запрос или откладывает его в диалог.
    pub(crate) fn handle_browser(
        &mut self,
        request: browser::BrowserRequest,
        reply: Sender<String>,
    ) {
        if !self.browser_enabled {
            let _ = reply.send(browser::error_json(
                "доступ из браузера выключен в настройках «Хранителя»",
            ));
            return;
        }
        // Пинг и генератор хранилища не требуют — отвечаем сразу.
        let needs_vault = !matches!(
            request,
            browser::BrowserRequest::Ping | browser::BrowserRequest::GeneratePassword
        );
        // Заблокировано — не отказываем сразу: поднимаем окно, просим
        // разблокировать и ответим, когда хранилище откроется.
        if needs_vault && self.vault.is_none() {
            self.pending_browser = Some((request, reply));
            self.focus_password = true;
            self.status = "Браузер просит доступ — введите мастер-пароль, запрос ждёт.".to_string();
            return;
        }
        match request {
            browser::BrowserRequest::Ping => {
                let _ = reply.send(browser::pong_json());
            }
            browser::BrowserRequest::GeneratePassword => {
                match generate_passphrase(&PassphraseOptions::default()) {
                    Ok(password) => {
                        let _ = reply.send(browser::password_json(&password));
                    }
                    Err(err) => {
                        let _ = reply.send(browser::error_json(&format!("генератор: {err}")));
                    }
                }
            }
            browser::BrowserRequest::GetLogins { url } => {
                if self.vault.is_none() {
                    let _ = reply.send(browser::error_json(
                        "хранилище заблокировано — введите мастер-пароль в «Хранителе»",
                    ));
                    return;
                }
                let domain = Self::site_domain(&url);
                // Без сайта в запросе непонятно, о каком доступе спрашивать:
                // пустой домен раньше сразу давал ответ без диалога.
                if domain.is_empty() {
                    let _ = reply.send(browser::error_json("не удалось определить сайт запроса"));
                    return;
                }
                if self.browser_domains.contains(&domain) {
                    let _ = reply.send(self.logins_reply(&url));
                } else {
                    self.browser_ask = Some(BrowserAsk::Access {
                        domain,
                        request: browser::BrowserRequest::GetLogins { url },
                        reply,
                    });
                }
            }
            browser::BrowserRequest::GetCredentials { id, url } => {
                if self.vault.is_none() {
                    let _ = reply.send(browser::error_json(
                        "хранилище заблокировано — введите мастер-пароль в «Хранителе»",
                    ));
                    return;
                }
                let domain = Self::site_domain(&url);
                // Тот же запрет: без адреса — никаких паролей и никаких диалогов.
                if domain.is_empty() {
                    let _ = reply.send(browser::error_json("не удалось определить сайт запроса"));
                    return;
                }
                if self.browser_domains.contains(&domain) {
                    let _ = reply.send(self.credentials_reply(id, &url));
                } else {
                    self.browser_ask = Some(BrowserAsk::Access {
                        domain,
                        request: browser::BrowserRequest::GetCredentials { id, url },
                        reply,
                    });
                }
            }
            browser::BrowserRequest::SaveLogin {
                url,
                username,
                password,
                title,
            } => {
                if self.vault.is_none() {
                    let _ = reply.send(browser::error_json(
                        "хранилище заблокировано — введите мастер-пароль в «Хранителе»",
                    ));
                    return;
                }
                // Есть ли уже запись этого сайта с тем же логином?
                let existing = self.vault.as_ref().and_then(|vault| {
                    vault
                        .items()
                        .iter()
                        .find(|item| {
                            !username.is_empty()
                                && item.username.eq_ignore_ascii_case(&username)
                                && item.matches_url(&url)
                        })
                        .map(|item| item.id)
                });
                match existing {
                    Some(id) => {
                        self.browser_ask = Some(BrowserAsk::SaveUpdate {
                            id,
                            url,
                            username,
                            password: Zeroizing::new(password),
                            title,
                            reply,
                        });
                    }
                    None => {
                        self.browser_ask = Some(BrowserAsk::SaveNew {
                            url,
                            username,
                            password: Zeroizing::new(password),
                            title,
                            reply,
                        });
                    }
                }
            }
        }
    }

    /// Список записей для сайта — без паролей.
    pub(crate) fn logins_reply(&mut self, url: &str) -> String {
        let Some(vault) = self.vault.as_ref() else {
            return browser::error_json("хранилище заблокировано");
        };
        let items: Vec<(Uuid, String, String, String)> = vault
            .items()
            .iter()
            .filter(|item| item.matches_url(url))
            .map(|item| {
                (
                    item.id,
                    item.title.clone(),
                    item.username.clone(),
                    item.all_urls()
                        .first()
                        .map(|u| u.to_string())
                        .unwrap_or_default(),
                )
            })
            .collect();
        self.status = format!(
            "Браузер: записей для {}: {}.",
            Self::site_domain(url),
            items.len()
        );
        browser::logins_json(url, &items)
    }

    /// Логин и пароль записи — только если запись относится к этому сайту.
    pub(crate) fn credentials_reply(&self, id: Uuid, url: &str) -> String {
        let Some(vault) = self.vault.as_ref() else {
            return browser::error_json("хранилище заблокировано");
        };
        let Some(item) = vault.get(id) else {
            return browser::error_json("запись не найдена");
        };
        // Адрес обязателен: без него нельзя проверить, что запись — про этот сайт.
        if !credentials_url_ok(url, item.matches_url(url)) {
            return browser::error_json("запись не относится к этому сайту");
        }
        browser::credentials_json(&item.username, &item.password)
    }

    /// Диалоги решений по запросам расширения браузера.
    pub(crate) fn browser_dialog(&mut self, ctx: &egui::Context) {
        let mut outcome: Option<BrowserOutcome> = None;
        let mut drawn = false;

        if let Some(ask) = self.browser_ask.as_ref() {
            drawn = true;
            match ask {
                BrowserAsk::Access { domain, .. } => {
                    egui::Window::new("Браузер просит доступ")
                        .collapsible(false)
                        .resizable(false)
                        .default_width(460.0)
                        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                        .show(ctx, |ui| {
                            ui.label(format!(
                                "Расширение браузера спрашивает записи для сайта {domain}."
                            ));
                            ui.label(
                                egui::RichText::new(
                                    "Разрешите, только если сами открыли вход на этом сайте.",
                                )
                                .weak()
                                .small(),
                            );
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if ui.button("Разрешить на этот раз").clicked() {
                                    outcome = Some(BrowserOutcome::AllowOnce);
                                }
                                if ui.button(format!("Всегда для {domain}")).clicked() {
                                    outcome = Some(BrowserOutcome::AllowAlways);
                                }
                                if ui.button("Отклонить").clicked() {
                                    outcome = Some(BrowserOutcome::Deny);
                                }
                            });
                        });
                }
                BrowserAsk::SaveNew { url, username, .. } => {
                    let host = Self::site_domain(url);
                    egui::Window::new("Браузер предлагает запись")
                        .collapsible(false)
                        .resizable(false)
                        .default_width(460.0)
                        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                        .show(ctx, |ui| {
                            ui.label(format!(
                                "Вход на сайте {host}: сохранить новую запись с логином «{username}»?"
                            ));
                            ui.label(
                                egui::RichText::new(
                                    "Проверьте, что сами только что входили с этими данными.",
                                )
                                .weak()
                                .small(),
                            );
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if ui.button("Сохранить").clicked() {
                                    outcome = Some(BrowserOutcome::Save);
                                }
                                if ui.button("Отклонить").clicked() {
                                    outcome = Some(BrowserOutcome::Deny);
                                }
                            });
                        });
                }
                BrowserAsk::SaveUpdate {
                    title,
                    url,
                    username,
                    ..
                } => {
                    let host = Self::site_domain(url);
                    let _ = username;
                    egui::Window::new("Браузер предлагает обновить пароль")
                        .collapsible(false)
                        .resizable(false)
                        .default_width(460.0)
                        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                        .show(ctx, |ui| {
                            ui.label(format!(
                                "Изменился пароль записи «{title}» для сайта {host}."
                            ));
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if ui.button("Обновить пароль").clicked() {
                                    outcome = Some(BrowserOutcome::SaveUpdate);
                                }
                                if ui.button("Сохранить как новую").clicked() {
                                    outcome = Some(BrowserOutcome::SaveAsNew);
                                }
                                if ui.button("Отклонить").clicked() {
                                    outcome = Some(BrowserOutcome::Deny);
                                }
                            });
                        });
                }
            }
        }
        if !drawn {
            return;
        }

        // Кнопка нажата — разбираемся с запросом и отвечаем расширению.
        let Some(outcome) = outcome else {
            return;
        };
        let Some(ask) = self.browser_ask.take() else {
            return;
        };
        self.apply_browser_outcome(ask, outcome);
    }

    /// Выполняет решение пользователя по запросу расширения.
    pub(crate) fn apply_browser_outcome(&mut self, ask: BrowserAsk, outcome: BrowserOutcome) {
        match (ask, outcome) {
            (BrowserAsk::Access { request, reply, .. }, BrowserOutcome::AllowOnce) => {
                let _ = reply.send(match request {
                    browser::BrowserRequest::GetLogins { url } => self.logins_reply(&url),
                    browser::BrowserRequest::GetCredentials { id, url } => {
                        self.credentials_reply(id, &url)
                    }
                    // Другие запросы в диалог доступа не попадают.
                    _ => browser::error_json("запрос устарел"),
                });
            }
            (
                BrowserAsk::Access {
                    domain,
                    request,
                    reply,
                },
                BrowserOutcome::AllowAlways,
            ) => {
                browser::remember_domain(&app_dir(), &domain);
                self.browser_domains.insert(domain.clone());
                self.status = format!("Браузер: сайт {domain} разрешён навсегда.");
                let _ = reply.send(match request {
                    browser::BrowserRequest::GetLogins { url } => self.logins_reply(&url),
                    browser::BrowserRequest::GetCredentials { id, url } => {
                        self.credentials_reply(id, &url)
                    }
                    _ => browser::error_json("запрос устарел"),
                });
            }
            (ask, BrowserOutcome::Deny) => {
                ask.deny("доступ отклонён пользователем");
            }
            (
                BrowserAsk::SaveNew {
                    url,
                    username,
                    password,
                    title,
                    reply,
                },
                BrowserOutcome::Save,
            ) => {
                let saved = self.browser_save_new(&url, &username, &password, &title);
                let _ = reply.send(saved);
            }
            (
                BrowserAsk::SaveUpdate {
                    id,
                    url,
                    username,
                    password,
                    title,
                    reply,
                },
                BrowserOutcome::SaveUpdate,
            ) => {
                let saved = self.browser_update_password(id, &password, &url);
                let _ = reply.send(saved);
                let _ = (username, title);
            }
            (
                BrowserAsk::SaveUpdate {
                    url,
                    username,
                    password,
                    title,
                    reply,
                    ..
                },
                BrowserOutcome::SaveAsNew,
            ) => {
                let saved = self.browser_save_new(&url, &username, &password, &title);
                let _ = reply.send(saved);
            }
            (ask, outcome) => {
                let _ = outcome;
                ask.deny("запрос отменён");
            }
        }
    }

    /// Создаёт запись из данных формы входа.
    pub(crate) fn browser_save_new(
        &mut self,
        url: &str,
        username: &str,
        password: &str,
        title: &str,
    ) -> String {
        let Some(vault) = self.vault.as_mut() else {
            return browser::error_json("хранилище заблокировано");
        };
        let host = Self::site_domain(url);
        let title = if title.trim().is_empty() {
            if host.is_empty() {
                "Новая запись"
            } else {
                &host
            }
        } else {
            title.trim()
        };
        let mut item = Item::new(ItemKind::Login, title);
        item.username = username.trim().to_string();
        item.password = password.to_string();
        item.url = url.trim().to_string();
        item.notes = auto_description(&item);
        let title = item.title.clone();
        match vault.add(item) {
            Ok(_) => {
                self.status = format!("Браузер: сохранена запись «{title}».");
                browser::saved_json(true, &title)
            }
            Err(err) => browser::error_json(&format!("не удалось сохранить: {err}")),
        }
    }

    /// Меняет пароль существующей записи.
    pub(crate) fn browser_update_password(
        &mut self,
        id: Uuid,
        password: &str,
        url: &str,
    ) -> String {
        let Some(vault) = self.vault.as_mut() else {
            return browser::error_json("хранилище заблокировано");
        };
        let Some(item) = vault.get(id) else {
            return browser::error_json("запись не найдена");
        };
        let mut updated = item.clone();
        updated.password = password.to_string();
        if updated.url.trim().is_empty() {
            updated.url = url.trim().to_string();
        }
        let title = updated.title.clone();
        match vault.update(updated) {
            Ok(()) => {
                self.status = format!("Браузер: пароль записи «{title}» обновлён.");
                browser::saved_json(false, &title)
            }
            Err(err) => browser::error_json(&format!("не удалось обновить: {err}")),
        }
    }
}
