# Установка «Хранителя» на этом компьютере из готового пакета.
#
# Что делает:
#   1. Копирует приложение в %LOCALAPPDATA%\Hranitel\app (или в -Dir).
#   2. Создаёт ярлык в меню «Пуск».
#   3. По желанию включает автозапуск (ключ Run, старт в трее).
#
# Что НЕ делает: не переносит хранилище и не ставит мост к браузеру —
# это два следующих шага, они напечатаются в конце.
#
# Примеры:
#   pwsh -File .\tools\install-hranitel.ps1
#   pwsh -File .\tools\install-hranitel.ps1 -Dir 'D:\Apps\Hranitel' -Autostart

param(
    # Куда установить. По умолчанию — профиль пользователя.
    [string]$Dir,

    # Включить запуск при входе в Windows (свёрнутым в трей).
    [switch]$Autostart,

    # Не создавать ярлык в меню «Пуск».
    [switch]$NoShortcut
)

$ErrorActionPreference = "Stop"

function Get-UserLocal {
    if ($env:LOCALAPPDATA) { return $env:LOCALAPPDATA }
    if ($env:USERPROFILE) { return (Join-Path $env:USERPROFILE "AppData\Local") }
    throw "не удалось определить LOCALAPPDATA"
}

# Пакет — это папка, где лежит этот скрипт рядом с vault-win.exe.
$source = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $source "vault-win.exe"))) {
    throw "рядом со скриптом нет vault-win.exe — запускайте tools\install-hranitel.ps1 из распакованного пакета"
}

if (-not $Dir) {
    $Dir = Join-Path (Get-UserLocal) "Hranitel\app"
}
New-Item -ItemType Directory -Force -Path $Dir | Out-Null

# Копируем всё, кроме самих скриптов установки и zip.
Get-ChildItem $source -Force | Where-Object {
    $_.Name -notin @("install-hranitel.ps1")
} | ForEach-Object {
    Copy-Item $_.FullName $Dir -Recurse -Force
}
Write-Host "Приложение установлено: $Dir"

$exe = Join-Path $Dir "vault-win.exe"

if (-not $NoShortcut) {
    $programs = [Environment]::GetFolderPath("Programs")
    if ($programs) {
        $shell = New-Object -ComObject WScript.Shell
        $link = $shell.CreateShortcut((Join-Path $programs "Хранитель.lnk"))
        $link.TargetPath = $exe
        $link.WorkingDirectory = $Dir
        $link.IconLocation = "$exe,0"
        $link.Description = "Локальный менеджер паролей"
        $link.Save()
        Write-Host "Ярлык: $programs\Хранитель.lnk"
    }
}

if ($Autostart) {
    $runKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
    New-Item -Path $runKey -Force | Out-Null
    Set-ItemProperty -Path $runKey -Name "Hranitel" -Value "`"$exe`" --tray --quiet"
    Write-Host "Автозапуск включён: $exe --tray --quiet"
}

Write-Host ""
Write-Host "Дальше:"
Write-Host "  1. Запустите «Хранитель» и откройте хранилище (или создайте новое)."
Write-Host "  2. Расширение: chrome://extensions (browser://extensions для Яндекса) ->"
Write-Host "     «Режим разработчика» -> «Загрузить распакованное расширение» ->"
Write-Host "     папка $Dir\extension"
Write-Host "  3. Мост к браузеру:"
Write-Host "     pwsh -File `"$Dir\tools\install-browser-host-auto.ps1`""
Write-Host "  4. Перезапустите браузер и щёлкните иконку расширения —"
Write-Host "     должно быть «Хранитель на связи»."
