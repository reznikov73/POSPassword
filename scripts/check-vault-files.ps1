# Проверяет, что в индексе git нет файлов хранилища «Хранителя».
#
# Файл хранилища узнаётся по сигнатуре OPVAULT1 в первых восьми байтах:
# расширение у него произвольное (vault.opvault, «Хранилище»), поэтому
# проверять по имени бесполезно — смотрим содержимое.
#
# Исключение — замороженные golden-фикстуры тестов совместимости
# (crates/vault-core/tests/fixtures): они добавлены намеренно.
#
# Используется в CI и в локальном pre-commit-хуке (.githooks/pre-commit).

param(
    [string[]]$Allow = @('crates/vault-core/tests/fixtures/')
)

$ErrorActionPreference = 'Stop'

$magic = 'OPVAULT1'

function Test-VaultFile {
    param([string]$Path)

    $full = Resolve-Path -LiteralPath $Path -ErrorAction SilentlyContinue
    if (-not $full) {
        return $false
    }
    $stream = [System.IO.File]::OpenRead($full)
    try {
        $head = New-Object byte[] 8
        $read = $stream.Read($head, 0, 8)
        if ($read -lt 8) {
            return $false
        }
        return [System.Text.Encoding]::ASCII.GetString($head) -eq $magic
    }
    finally {
        $stream.Dispose()
    }
}

$tracked = & git -c core.quotepath=false ls-files
if ($LASTEXITCODE -ne 0) {
    Write-Error 'Не удалось получить список файлов из git ls-files.'
    exit 1
}

$bad = @()
foreach ($file in $tracked) {
    $normalized = $file -replace '\\', '/'
    $allowed = $false
    foreach ($prefix in $Allow) {
        if ($normalized.StartsWith($prefix)) {
            $allowed = $true
            break
        }
    }
    if ($allowed) {
        continue
    }
    if (Test-VaultFile -Path $file) {
        $bad += $file
    }
}

if ($bad.Count -gt 0) {
    Write-Host 'В репозитории найдены файлы хранилища паролей:' -ForegroundColor Red
    foreach ($file in $bad) {
        Write-Host "  $file" -ForegroundColor Red
    }
    Write-Host 'Хранилища не коммитятся: уберите файл из индекса (git rm --cached).'
    exit 1
}

Write-Host 'Файлов хранилища в индексе нет.'
exit 0
