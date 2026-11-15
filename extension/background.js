// Фоновый сервис-воркер расширения «Хранитель».
//
// Вся связь с приложением — через родной хост (Native Messaging):
// расширение открывает порт, браузер запускает browser-host.exe, тот
// подключается к «Хранителю» по именованному каналу. Секреты в браузере
// не хранятся: каждый запрос идёт в приложение, ответы приходят оттуда.
//
// Сервис-воркер MV3 может засыпать, поэтому порт открывается на один
// запрос и закрывается после ответа — переживать сон воркера не нужно.

const HOST_NAME = "ru.hranitel.bridge";
const REPLY_TIMEOUT_MS = 125_000;

// Сообщения от content-скрипта и попапа: {type, ...}.
chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (!message || typeof message !== "object") {
    return false;
  }
  switch (message.type) {
    case "ping":
      exchange({ action: "ping" }).then(sendResponse);
      return true;
    case "get-logins":
      exchange({ action: "get-logins", url: message.url }).then(sendResponse);
      return true;
    case "get-credentials":
      exchange({ action: "get-credentials", id: message.id, url: message.url }).then(
        sendResponse,
      );
      return true;
    case "save-login":
      exchange({
        action: "save-login",
        url: message.url,
        username: message.username,
        password: message.password,
        title: message.title,
      }).then(sendResponse);
      return true;
    case "generate-password":
      exchange({ action: "generate-password" }).then(sendResponse);
      return true;
    default:
      return false;
  }
});

/// Обменивается одним сообщением с приложением через родной хост.
/// Всегда возвращает объект-ответ (или ошибку в едином формате).
async function exchange(request) {
  return new Promise((resolve) => {
    let port;
    try {
      port = chrome.runtime.connectNative(HOST_NAME);
    } catch (err) {
      resolve(errorReply(`не удалось запустить мост: ${err}`));
      return;
    }

    const timer = setTimeout(() => {
      try {
        port.disconnect();
      } catch (_err) {
        /* порт уже закрыт */
      }
      resolve(errorReply("«Хранитель» не ответил за 2 минуты"));
    }, REPLY_TIMEOUT_MS);

    port.onMessage.addListener((reply) => {
      clearTimeout(timer);
      try {
        port.disconnect();
      } catch (_err) {
        /* уже закрыт */
      }
      resolve(reply ?? errorReply("пустой ответ"));
    });

    port.onDisconnect.addListener(() => {
      clearTimeout(timer);
      const err = chrome.runtime.lastError?.message ?? "мост отключился";
      resolve(errorReply(`«Хранитель» недоступен (${err})`));
    });

    port.postMessage(request);
  });
}

function errorReply(message) {
  return { action: "error", message };
}
