# Собирает готовый пакет в dist\Hranitel-<версия>-win64 и zip рядом с ним.
# Запуск: pwsh -File scripts\package.ps1
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

    cargo build --release -p vault-win -p vault-cli -p login-probe -p browser-host
    if ($LASTEXITCODE -ne 0) { throw 'сборка не удалась' }

    $dst = Join-Path 'dist' "Hranitel-$Version-win64"
    if (Test-Path $dst) { Remove-Item -Recurse -Force $dst }
    New-Item -ItemType Directory $dst | Out-Null

    Copy-Item 'target\release\vault-win.exe', 'target\release\vault-cli.exe', 'README.md' $dst
    Copy-Item 'docs' $dst -Recurse

    # Программа-мишень для проверки автоввода — отдельной папкой.
    $tools = Join-Path $dst 'tools'
    New-Item -ItemType Directory $tools -Force | Out-Null
    Copy-Item 'target\release\login-probe.exe', 'tools\login-probe\README.md' $tools

    # Мост к браузеру: родной хост и распакованное расширение.
    Copy-Item 'target\release\browser-host.exe' $dst
    Copy-Item 'tools\install-browser-host.ps1', 'tools\install-browser-host-auto.ps1' $tools
    Copy-Item 'extension' $dst -Recurse

    $zip = Join-Path 'dist' "Hranitel-$Version-win64.zip"
    if (Test-Path $zip) { Remove-Item -Force $zip }
    Compress-Archive -Path $dst -DestinationPath $zip

    Write-Output "Готово: $dst"
    Write-Output "Архив:  $zip"
} finally {
    Pop-Location
}
