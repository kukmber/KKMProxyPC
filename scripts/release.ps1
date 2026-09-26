<#
.SYNOPSIS
Собирает подписанный выпуск и публикует его на GitHub.

.DESCRIPTION
Делает всё, что нужно для того, чтобы у пользователей сработало обновление
внутри программы:

  1. собирает установщик, подписывая его ключом обновлений;
  2. создаёт релиз с тегом vX.Y.Z;
  3. прикладывает установщик, его подпись и файл latest.json.

Без latest.json программа не узнает о новой версии — его читает встроенный
механизм обновления.

.PARAMETER Token
Токен GitHub с правом записи в репозиторий.

.PARAMETER KeyPath
Закрытый ключ обновлений. По умолчанию ~\.kkmproxy\updater.key.
Ключ нельзя терять: без него подписать обновление уже не выйдет.

.PARAMETER Notes
Описание выпуска. Если не задано, берётся строка о версии.

.EXAMPLE
.\scripts\release.ps1 -Token github_pat_xxx
#>
param(
  [Parameter(Mandatory = $true)][string]$Token,
  [string]$KeyPath = "$env:USERPROFILE\.kkmproxy\updater.key",
  [string]$Notes = "",
  [string]$Repo = "kukmber/KKMProxyPC",
  [switch]$SkipBuild
)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

if (-not (Test-Path $KeyPath)) { throw "Нет ключа обновлений: $KeyPath" }
$conf = Get-Content "$root\src-tauri\tauri.conf.json" -Raw | ConvertFrom-Json
$version = $conf.version
$tag = "v$version"
"версия: $version"

if (-not $SkipBuild) {
  # Щадящие настройки: полный LTO требует много памяти и часто не помещается.
  $env:CARGO_PROFILE_RELEASE_LTO = "thin"
  $env:CARGO_PROFILE_RELEASE_CODEGEN_UNITS = "16"
  # Tauri ждёт сам ключ в переменной, а не путь к нему.
  $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $KeyPath -Raw).Trim()
  $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
  npm run tauri build
  if ($LASTEXITCODE -ne 0) { throw "сборка не удалась" }
}

$setup = Get-ChildItem "$root\src-tauri\target\release\bundle\nsis\*-setup.exe" | Select-Object -First 1
$sig = "$($setup.FullName).sig"
if (-not (Test-Path $sig)) { throw "Нет подписи $sig — собирайте с ключом обновлений" }
"установщик: $($setup.Name), $([math]::Round($setup.Length/1MB,2)) МБ"

$h = @{ Authorization = "Bearer $Token"; 'User-Agent' = 'kkm'; Accept = 'application/vnd.github+json' }
$api = "https://api.github.com/repos/$Repo"

# Старый релиз с тем же тегом мешает: заменяем его целиком.
try {
  $old = Invoke-RestMethod "$api/releases/tags/$tag" -Headers $h
  Invoke-RestMethod "$api/releases/$($old.id)" -Headers $h -Method Delete | Out-Null
  "прежний релиз $tag удалён"
} catch {}

if (-not $Notes) { $Notes = "KKMProxy для Windows $version" }
$body = @{ tag_name = $tag; name = "KKMProxy для Windows $version"; body = $Notes; draft = $false; prerelease = $false } | ConvertTo-Json -Depth 3
$rel = Invoke-RestMethod "$api/releases" -Headers $h -Method Post -Body ([Text.Encoding]::UTF8.GetBytes($body)) -ContentType 'application/json; charset=utf-8'
"релиз создан: $($rel.html_url)"

function Send-Asset([string]$path, [string]$name) {
  $url = ($rel.upload_url -replace '\{.*\}', '') + "?name=$name"
  $a = Invoke-RestMethod $url -Headers $h -Method Post -InFile $path -ContentType 'application/octet-stream'
  "  приложено: $($a.name)"
}

Send-Asset $setup.FullName $setup.Name
Send-Asset $sig "$($setup.Name).sig"

# latest.json — то, что программа читает при проверке обновления.
$latest = @{
  version   = $version
  notes     = $Notes
  pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  platforms = @{
    'windows-x86_64' = @{
      signature = (Get-Content $sig -Raw).Trim()
      url       = "https://github.com/$Repo/releases/download/$tag/$($setup.Name)"
    }
  }
} | ConvertTo-Json -Depth 5
$tmp = Join-Path $env:TEMP "latest.json"
[IO.File]::WriteAllBytes($tmp, [Text.Encoding]::UTF8.GetBytes($latest))
Send-Asset $tmp "latest.json"

"готово: $($rel.html_url)"
