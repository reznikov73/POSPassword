# Собирает установщик dist\Hranitel-<версия>-setup.exe (Inno Setup 6).
#
# Перед сборкой нужен готовый пакет dist\Hranitel-<версия>-win64 —
# если его нет, скрипт соберёт его сам через scripts\package.ps1.
#
# Пример:
#   pwsh -File scripts\build-installer.ps1
#   pwsh -File scripts\build-installer.ps1 -Version 0.12.2

param(
    [string]$Version
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    if (-not $Version) {
        $line = Select-String -Path 'Cargo.toml' -Pattern '^version = "(.+)"' | Select-Object -First 1
        if (-not $line) { throw 'не удалось определить версию из Cargo.toml' }
        $Version = $line.Matches[0].Groups[1].Value
    }

    $package = "dist\Hranitel-$Version-win64"
    if (-not (Test-Path (Join-Path $package 'vault-win.exe'))) {
        Write-Host "Пакет $package не найден — собираю…"
        pwsh -File scripts\package.ps1 -Version $Version
    }

    if (-not (Test-Path 'installer\hranitel.ico')) {
        throw 'нет installer\hranitel.ico — иконка нужна для установщика'
    }

    $iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    if (-not $iscc) {
        throw 'ISCC.exe не найден: установите Inno Setup 6 (choco install innosetup)'
    }

    & $iscc "/DMyAppVersion=$Version" 'installer\hranitel.iss'
    if ($LASTEXITCODE -ne 0) { throw "iscc завершился с кодом $LASTEXITCODE" }

    $setup = "dist\Hranitel-$Version-setup.exe"
    if (-not (Test-Path $setup)) { throw "установщик не появился: $setup" }

    $file = Get-Item $setup
    Write-Output "Установщик: $($file.FullName)"
    Write-Output ("Размер:     {0:N1} МиБ" -f ($file.Length / 1MB))
} finally {
    Pop-Location
}
