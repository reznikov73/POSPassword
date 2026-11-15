# Установка моста «Хранитель — браузер» (Native Messaging).
#
# Что делает скрипт:
#  1. Копирует манифест родного хоста в %APPDATA%\Hranitel\browser-host и
#     подставляет в него абсолютный путь к browser-host.exe и идентификатор
#     расширения.
#  2. Прописывает путь к манифесту в реестре (текущий пользователь, без
#     прав администратора) для выбранных браузеров: Chrome и/или Edge.
#
# Пример:
#   .\tools\install-browser-host.ps1 -ExePath .\target\release\browser-host.exe `
#       -ExtensionId abcdefghijklmnopabcdefghijklmnop -Browsers chrome,edge
#
# Идентификатор расширения виден на странице chrome://extensions после
# загрузки распакованного расширения (Режим разработчика → Загрузить
# распакованное расширение → папка extension).

param(
    [Parameter(Mandatory = $true)]
    [string]$ExePath,

    [Parameter(Mandatory = $true)]
    [string]$ExtensionId,

    [string[]]$Browsers = @("chrome", "edge")
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $ExePath)) {
    Write-Error "Не найден browser-host.exe: $ExePath. Соберите его: cargo build --release -p browser-host"
}
$ExePath = (Resolve-Path $ExePath).Path

if ($ExtensionId -notmatch '^[a-p]{32}$') {
    Write-Warning "Идентификатор расширения выглядит необычно (обычно 32 буквы a–p): $ExtensionId"
}

$targetDir = Join-Path $(if ($env:APPDATA) { $env:APPDATA } else { Join-Path $env:USERPROFILE "AppData\Roaming" }) "Hranitel\browser-host"
New-Item -ItemType Directory -Force -Path $targetDir | Out-Null

$manifest = @{
    name            = "ru.hranitel.bridge"
    description     = "Хранитель — локальное хранилище паролей"
    path            = $ExePath
    type            = "stdio"
    allowed_origins = @("chrome-extension://$ExtensionId/")
}
$manifestPath = Join-Path $targetDir "ru.hranitel.bridge.json"
$manifest | ConvertTo-Json | Set-Content -Path $manifestPath -Encoding UTF8
Write-Host "Манифест хоста: $manifestPath"

foreach ($browser in $Browsers) {
    switch ($browser) {
        "chrome" { $key = "HKCU:\Software\Google\Chrome\NativeMessagingHosts\ru.hranitel.bridge" }
        "edge" { $key = "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\ru.hranitel.bridge" }
        # Яндекс Браузер — Chromium-сборка и ищет хосты по ключу Chrome
        # (в browser.dll: SOFTWARE\Google\Chrome\NativeMessagingHosts).
        "yandex" { $key = "HKCU:\Software\Google\Chrome\NativeMessagingHosts\ru.hranitel.bridge" }
        default { Write-Warning "Неизвестный браузер: $browser (поддерживаются chrome, edge, yandex)"; continue }
    }
    New-Item -Path $key -Force | Out-Null
    Set-ItemProperty -Path $key -Name "(default)" -Value $manifestPath
    Write-Host "Реестр ($browser): $key"
}

Write-Host ""
Write-Host "Готово. Порядок проверки:"
Write-Host "  1. Запустите «Хранитель» и откройте хранилище."
Write-Host "  2. В браузере загрузите папку extension как распакованное расширение."
Write-Host "  3. Откройте сайт из записей хранилища и щёлкните иконку в поле пароля."
