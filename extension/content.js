// Content-скрипт «Хранителя»: иконка в полях входа, список записей,
// заполнение по клику и предложение сохранить новый пароль.
//
// Принципы те же, что у KeePassXC и 1Password:
//  - ничего не заполняется само при загрузке страницы — только по клику;
//  - пароли не хранятся в браузере: список и данные приходят из приложения;
//  - сохранение нового пароля подтверждается в самом «Хранителе».

(() => {
  const FILL_KEY = "hranitelFill";
  const SAVE_KEY = "hranitelSave";

  // Последнее заполненное на странице — чтобы не предлагать сохранить
  // то, что сами же и наполнили.
  let lastFilled = null;

  // Активное поле пароля, для которого показана иконка.
  let activeField = null;

  const ui = {
    icon: null,
    list: null,
  };

  // ---------- Запросы к приложению ----------

  function send(message) {
    return new Promise((resolve) => {
      chrome.runtime.sendMessage(message, (reply) => {
        resolve(reply ?? { action: "error", message: "нет ответа" });
      });
    });
  }

  // ---------- Настройки ----------

  async function settings() {
    return new Promise((resolve) => {
      chrome.storage.local.get([FILL_KEY, SAVE_KEY], (stored) => {
        resolve({
          fill: stored[FILL_KEY] !== false,
          save: stored[SAVE_KEY] !== false,
        });
      });
    });
  }

  // ---------- Поиск полей ----------

  function visible(field) {
    const rect = field.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) {
      return false;
    }
    const style = window.getComputedStyle(field);
    return style.visibility !== "hidden" && style.display !== "none";
  }

  function passwordFields() {
    return [...document.querySelectorAll("input[type=password]")].filter(visible);
  }

  /// Ищет поле логина рядом с паролем: сначала в той же форме перед
  /// паролем, потом по всему документу. Эвристика как у KeePassXC:
  /// подсказки autocomplete, знакомые имена, типы полей.
  function usernameField(password) {
    const scope = password.form ?? document;
    const candidates = [...scope.querySelectorAll("input")].filter((field) => {
      if (field === password || field.type === "password" || field.type === "hidden") {
        return false;
      }
      if (!["text", "email", "tel"].includes(field.type)) {
        return false;
      }
      return visible(field);
    });

    // Сначала — с подсказкой про логин.
    const hinted = candidates.find((field) => {
      const hints = [
        field.autocomplete,
        field.name,
        field.id,
        field.placeholder,
      ]
        .join(" ")
        .toLowerCase();
      return /user|login|email|mail|phone|телефон|почт|логин/.test(hints);
    });
    if (hinted) {
      return hinted;
    }
    // Ближайшее текстовое поле перед паролем.
    const before = candidates.filter(
      (field) => field.compareDocumentPosition(password) & Node.DOCUMENT_POSITION_FOLLOWING,
    );
    return before.length ? before[before.length - 1] : null;
  }

  // ---------- Иконка в поле ----------

  function hideIcon() {
    if (ui.icon) {
      ui.icon.remove();
      ui.icon = null;
    }
    activeField = null;
  }

  function showIcon(field) {
    if (!ui.icon) {
      ui.icon = document.createElement("div");
      ui.icon.textContent = "🔑";
      ui.icon.title = "Хранитель: выбрать запись";
      Object.assign(ui.icon.style, {
        position: "fixed",
        zIndex: 2147483647,
        width: "22px",
        height: "22px",
        lineHeight: "22px",
        textAlign: "center",
        borderRadius: "6px",
        background: "rgba(38, 92, 180, 0.92)",
        color: "#fff",
        fontSize: "13px",
        cursor: "pointer",
        userSelect: "none",
      });
      ui.icon.addEventListener("mousedown", (event) => {
        event.preventDefault();
        event.stopPropagation();
      });
      ui.icon.addEventListener("click", (event) => {
        event.preventDefault();
        event.stopPropagation();
        openList(field);
      });
      document.documentElement.appendChild(ui.icon);
    }
    const rect = field.getBoundingClientRect();
    ui.icon.style.left = `${rect.right - 26}px`;
    ui.icon.style.top = `${rect.top + (rect.height - 22) / 2}px`;
    activeField = field;
  }

  // ---------- Список записей ----------

  function hideList() {
    if (ui.list) {
      ui.list.remove();
      ui.list = null;
    }
  }

  async function openList(field) {
    hideList();
    const opts = await settings();
    if (!opts.fill) {
      return;
    }

    const anchor = field.getBoundingClientRect();
    // Два меню одновременно — это баг: если предыдущее осталось, убираем.
    if (ui.list) {
      ui.list.remove();
      ui.list = null;
    }
    ui.list = document.createElement("div");
    Object.assign(ui.list.style, {
      position: "fixed",
      zIndex: 2147483647,
      left: `${Math.max(8, anchor.left)}px`,
      top: `${Math.min(anchor.bottom + 6, window.innerHeight - 220)}px`,
      minWidth: "260px",
      maxWidth: "360px",
      background: "#fff",
      color: "#1a1d23",
      border: "1px solid #7c808f",
      borderRadius: "10px",
      boxShadow: "0 8px 24px rgba(0,0,0,.25)",
      fontFamily: "Segoe UI, Arial, sans-serif",
      fontSize: "13px",
      padding: "4px",
    });

    const header = document.createElement("div");
    header.textContent = "Хранитель";
    Object.assign(header.style, {
      padding: "6px 8px 4px",
      fontWeight: "600",
      color: "#265cb4",
    });
    ui.list.appendChild(header);

    const reply = await send({ type: "get-logins", url: location.href });
    if (reply.action === "logins") {
      if (!reply.items.length) {
        const empty = document.createElement("div");
        empty.textContent = "Для этого сайта записей нет";
        Object.assign(empty.style, { padding: "6px 8px 10px", color: "#5c6270" });
        ui.list.appendChild(empty);
      }
      for (const item of reply.items) {
        const row = document.createElement("div");
        const title = document.createElement("div");
        title.textContent = item.title;
        Object.assign(title.style, { fontWeight: "600" });
        const login = document.createElement("div");
        login.textContent = item.username || item.url || "";
        Object.assign(login.style, { color: "#5c6270", fontSize: "12px" });
        row.append(title, login);
        Object.assign(row.style, { padding: "6px 8px", borderRadius: "6px", cursor: "pointer" });
        row.addEventListener("mouseenter", () => (row.style.background = "#dbe7fa"));
        row.addEventListener("mouseleave", () => (row.style.background = "transparent"));
        row.addEventListener("click", () => {
          hideList();
          fillCredentials(item, field);
        });
        ui.list.appendChild(row);
      }
    } else {
      const error = document.createElement("div");
      error.textContent = reply.message ?? "Хранитель недоступен";
      Object.assign(error.style, { padding: "6px 8px 10px", color: "#b4342f" });
      ui.list.appendChild(error);
    }

    // Генератор прямо в списке.
    const gen = document.createElement("div");
    gen.textContent = "Сгенерировать пароль";
    Object.assign(gen.style, {
      padding: "6px 8px",
      marginTop: "4px",
      borderTop: "1px solid #e2e7ee",
      color: "#265cb4",
      cursor: "pointer",
      borderRadius: "6px",
    });
    gen.addEventListener("click", async () => {
      hideList();
      const password = await send({ type: "generate-password" });
      if (password.action === "password") {
        fillPassword(field, password.value);
        toast(`Пароль сгенерирован: ${password.value}`);
      } else {
        toast(password.message ?? "Генератор недоступен");
      }
    });
    ui.list.appendChild(gen);

    document.documentElement.appendChild(ui.list);
    setTimeout(() => {
      document.addEventListener("mousedown", onOutside, { once: true });
    });
  }

  function onOutside(event) {
    if (ui.list && !ui.list.contains(event.target) && event.target !== ui.icon) {
      hideList();
    }
  }

  // ---------- Заполнение ----------

  async function fillCredentials(item, passwordField) {
    const reply = await send({
      type: "get-credentials",
      id: item.id,
      url: location.href,
    });
    if (reply.action !== "credentials") {
      toast(reply.message ?? "Не удалось получить запись");
      return;
    }
    const login = usernameField(passwordField);
    if (login && reply.username) {
      setNativeValue(login, reply.username);
    }
    fillPassword(passwordField, reply.password);
    lastFilled = {
      username: reply.username,
      password: reply.password,
    };
    toast(`Заполнено: ${item.title}`);
  }

  function fillPassword(field, value) {
    setNativeValue(field, value);
    // Форма смены пароля часто требует повторить новый пароль.
    const form = field.form;
    if (form) {
      const repeats = [...form.querySelectorAll("input[type=password]")].filter(
        (candidate) => candidate !== field && !candidate.value,
      );
      for (const repeat of repeats) {
        setNativeValue(repeat, value);
      }
    }
  }

  /// Меняет значение поля так, как будто его набрал человек: событие input
  /// видят React- и Vue-страницы, change — обычные формы.
  function setNativeValue(field, value) {
    const setter = Object.getOwnPropertyDescriptor(
      field.__proto__,
      "value",
    )?.set;
    if (setter) {
      setter.call(field, value);
    } else {
      field.value = value;
    }
    field.dispatchEvent(new Event("input", { bubbles: true }));
    field.dispatchEvent(new Event("change", { bubbles: true }));
  }

  // ---------- Сохранение нового входа ----------

  async function offerSave(form) {
    const opts = await settings();
    if (!opts.save) {
      return;
    }
    const passwords = [...form.querySelectorAll("input[type=password]")].filter(visible);
    if (!passwords.length) {
      return;
    }
    const password = passwords[0].value;
    if (!password) {
      return;
    }
    const login = usernameField(passwords[0]);
    const username = login?.value ?? "";
    if (lastFilled && lastFilled.username === username && lastFilled.password === password) {
      return; // сами наполнили — сохранять нечего
    }
    const title = document.title.split(/[|·—–]/)[0].trim() || location.hostname;
    const reply = await send({
      type: "save-login",
      url: location.href,
      username,
      password,
      title,
    });
    if (reply.action === "saved") {
      lastFilled = { username, password };
    }
  }

  // ---------- Мелочи ----------

  function toast(text) {
    const note = document.createElement("div");
    note.textContent = text;
    Object.assign(note.style, {
      position: "fixed",
      zIndex: 2147483647,
      right: "16px",
      bottom: "16px",
      maxWidth: "320px",
      background: "#1e2228",
      color: "#fff",
      padding: "10px 14px",
      borderRadius: "10px",
      fontFamily: "Segoe UI, Arial, sans-serif",
      fontSize: "13px",
      boxShadow: "0 6px 18px rgba(0,0,0,.3)",
    });
    document.documentElement.appendChild(note);
    setTimeout(() => note.remove(), 3500);
  }

  // ---------- События ----------

  document.addEventListener(
    "focusin",
    async (event) => {
      const field = event.target;
      if (!(field instanceof HTMLInputElement)) {
        return;
      }
      const opts = await settings();
      if (!opts.fill) {
        return;
      }
      if (field.type === "password") {
        showIcon(field);
      } else if (activeField && field !== activeField) {
        hideIcon();
      }
    },
    true,
  );

  window.addEventListener("scroll", hideIcon, true);
  window.addEventListener("resize", hideIcon);

  // Отправка формы — момент предложить сохранить новый или изменённый вход.
  document.addEventListener(
    "submit",
    (event) => {
      if (event.target instanceof HTMLFormElement) {
        offerSave(event.target);
      }
    },
    true,
  );
})();
