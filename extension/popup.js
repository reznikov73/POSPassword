// Попап: проверка моста и переключатели поведения на страницах.

const statusBox = document.getElementById("status");
const fillBox = document.getElementById("fill");
const saveBox = document.getElementById("save");

// Состояние моста: живой пинг до приложения через родной хост.
chrome.runtime.sendMessage({ type: "ping" }, (reply) => {
  if (reply && reply.action === "pong") {
    statusBox.className = "status ok";
    statusBox.textContent = `Хранитель на связи (версия ${reply.version})`;
  } else {
    statusBox.className = "status err";
    statusBox.textContent =
      reply?.message ?? "Хранитель недоступен — запустите приложение";
  }
});

// Настройки читаем и пишем в локальное хранилище расширения.
chrome.storage.local.get(["hranitelFill", "hranitelSave"], (stored) => {
  fillBox.checked = stored.hranitelFill !== false;
  saveBox.checked = stored.hranitelSave !== false;
});

fillBox.addEventListener("change", () => {
  chrome.storage.local.set({ hranitelFill: fillBox.checked });
});

saveBox.addEventListener("change", () => {
  chrome.storage.local.set({ hranitelSave: saveBox.checked });
});
