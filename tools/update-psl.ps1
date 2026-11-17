# Обновляет встроенный снапшот Public Suffix List (Б-1 из ТЗ 0.13.0).
#
# Кладёт свежий список в crates/vault-core/src/psl_suffixes.dat.
# После обновления: cargo test -p vault-core (тесты PSL ловят регрессии).
#
# Запуск: pwsh -File tools\update-psl.ps1
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$dst = Join-Path $root 'crates\vault-core\src\psl_suffixes.dat'
$url = 'https://publicsuffix.org/list/public_suffix_list.dat'

Write-Output "Скачиваю $url ..."
$bytes = (Invoke-WebRequest -Uri $url -UseBasicParsing).Content
[System.IO.File]::WriteAllBytes($dst, $bytes)

$size = (Get-Item $dst).Length
Write-Output "Готово: $dst ($([math]::Round($size / 1KB)) КиБ)"
Write-Output 'Теперь проверьте: cargo test -p vault-core && cargo clippy -p vault-core --all-targets -- -D warnings'
