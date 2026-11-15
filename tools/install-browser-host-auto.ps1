# Установка моста «Хранитель — браузер» без ручного копирования ID.
#
# Скрипт сам находит идентификатор распакованного расширения в профилях
# Chrome/Edge (файл Preferences, запись с location=4 и нашим путём) и
# вызывает tools\install-browser-host.ps1 с нужными параметрами.
#
# Порядок:
#   1. Загрузите папку extension как распакованное расширение.
#   2. Закройте браузер (Chrome пишет Preferences при изменениях и на выходе).
#   3. Запустите этот скрипт.
#
# Пример:
#   pwsh -File .\tools\install-browser-host-auto.ps1
#   pwsh -File .\tools\install-browser-host-auto.ps1 -Browsers chrome -DryRun

param(
    [string[]]$Browsers = @("chrome", "edge", "yandex"),

    # Папка расширения; по умолчанию — ..\extension рядом со скриптом.
    [string]$ExtensionDir,

    # Готовый browser-host.exe; по умолчанию — рядом со скриптом или в корне пакета.
    [string]$HostExe,

    # Только показать найденный ID, ничего не устанавливать.
    [switch]$DryRun,

    # Для проверки: разобрать конкретный файл Preferences вместо профилей.
    [string]$PreferencesPath
)

$ErrorActionPreference = "Stop"

function Get-LocalAppData {
    if ($env:LOCALAPPDATA) { return $env:LOCALAPPDATA }
    if ($env:USERPROFILE) { return (Join-Path $env:USERPROFILE "AppData\Local") }
    throw "не удалось определить LOCALAPPDATA"
}

function Resolve-ExtensionDir([string]$path) {
    if (-not $path) {
        $path = Join-Path (Split-Path -Parent $PSScriptRoot) "extension"
    }
    if (-not (Test-Path $path)) {
        throw "не найдена папка расширения: $path"
    }
    return (Resolve-Path $path).Path.TrimEnd('\')
}

# Сравнивает пути без учёта регистра и лишних слэшей.
function Test-SamePath([string]$left, [string]$right) {
    if (-not $left -or -not $right) { return $false }
    $a = $left.Replace('/', '\').TrimEnd('\')
    $b = $right.Replace('/', '\').TrimEnd('\')
    return [string]::Equals($a, $b, [System.StringComparison]::OrdinalIgnoreCase)
}

# Ищет в Preferences/Secure Preferences распакованные расширения (location=4)
# с нашим путём. Secure Preferences содержит ключи, различающиеся регистром,
# поэтому разбор — в хэш-таблицу, иначе ConvertFrom-Json падает.
function Find-UnpackedExtensionId([string]$prefsFile, [string]$extensionDir) {
    if (-not (Test-Path $prefsFile)) { return $null }
    try {
        $json = Get-Content -Raw -Encoding utf8 $prefsFile | ConvertFrom-Json -AsHashtable
    } catch {
        return $null
    }
    if (-not $json -or -not $json.ContainsKey('extensions')) { return $null }
    $extensions = $json['extensions']
    if (-not $extensions -or -not $extensions.ContainsKey('settings')) { return $null }

    foreach ($entry in $extensions['settings'].GetEnumerator()) {
        $value = $entry.Value
        if (-not $value) { continue }
        if ($value.ContainsKey('location') -and $value['location'] -ne 4) { continue }
        if (-not (Test-SamePath $value['path'] $extensionDir)) { continue }
        $id = $entry.Key
        if ($id -match '^[a-p]{32}$') { return $id }
    }
    return $null
}

function Get-ProfileFiles([string]$browser) {
    $local = Get-LocalAppData
    $root = switch ($browser) {
        "chrome" { Join-Path $local "Google\Chrome\User Data" }
        "edge" { Join-Path $local "Microsoft\Edge\User Data" }
        "yandex" { Join-Path $local "Yandex\YandexBrowser\User Data" }
        default { $null }
    }
    if (-not $root -or -not (Test-Path $root)) { return @() }
    $files = @()
    $profiles = @("Default") + @(Get-ChildItem $root -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -like "Profile *" } | ForEach-Object { $_.Name })
    foreach ($profile in $profiles) {
        # Настройки расширений Chromium держит в Secure Preferences,
        # а обычный Preferences — только общие настройки.
        $files += Join-Path $root "$profile\Secure Preferences"
        $files += Join-Path $root "$profile\Preferences"
    }
    return $files | Where-Object { Test-Path $_ }
}

$extensionDir = Resolve-ExtensionDir $ExtensionDir

if ($PreferencesPath) {
    $found = Find-UnpackedExtensionId $PreferencesPath $extensionDir
    if ($found) {
        Write-Output "ID: $found"
        exit 0
    }
    Write-Output "расширение с путём $extensionDir в файле не найдено"
    exit 1
}

$hostPath = $HostExe
if (-not $hostPath) {
    $candidates = @(
        (Join-Path $PSScriptRoot "..\browser-host.exe"),
        (Join-Path $PSScriptRoot "browser-host.exe"),
        (Join-Path (Split-Path -Parent $PSScriptRoot) "target\release\browser-host.exe")
    )
    $hostPath = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $hostPath) {
    throw "не найден browser-host.exe: укажите -HostExe"
}

$installer = Join-Path $PSScriptRoot "install-browser-host.ps1"
if (-not (Test-Path $installer)) {
    throw "не найден install-browser-host.ps1 рядом со скриптом"
}

$foundAny = $false
foreach ($browser in $Browsers) {
    $id = $null
    $profile = $null
    foreach ($file in Get-ProfileFiles $browser) {
        $id = Find-UnpackedExtensionId $file $extensionDir
        if ($id) { $profile = $file; break }
    }
    if (-not $id) {
        Write-Warning "$browser`: расширение с путём $extensionDir не найдено в профиле. Загрузите его и закройте браузер."
        continue
    }
    $foundAny = $true
    Write-Host "$browser`: ID $id (профиль $profile)"
    if ($DryRun) { continue }
    & $installer -ExePath $hostPath -ExtensionId $id -Browsers $browser
}

if (-not $foundAny) {
    Write-Host ""
    Write-Host "Ничего не установлено. Что сделать:"
    Write-Host "  1. chrome://extensions → «Режим разработчика» → «Загрузить распакованное расширение» → $extensionDir"
    Write-Host "  2. закрыть браузер (Preferences пишется на выходе)"
    Write-Host "  3. повторить этот скрипт"
    exit 1
}
