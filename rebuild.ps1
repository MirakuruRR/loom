#requires -Version 5.1
<#
.SYNOPSIS
  Пересобирает loom.exe и loom-mcp.exe из этого репозитория и обновляет симлинки кита.

.DESCRIPTION
  Кит Whiskerwood держит Plugins\LoomEditor симлинком на out\<slug>\plugin\LoomEditor
  этого репозитория, а внутри Binaries\ThirdParty\Loom\Win64\loom.exe и loom-mcp.exe —
  симлинками на target\<профиль>\. Скрипт собирает бинарники и чинит эти ссылки.

  Почему нельзя просто перезапустить Claude Code: симлинк отдаёт тот же файл, а
  запущенный MCP-сервер loom держит target\debug\loom-mcp.exe, из-за чего cargo падает
  на записи бинарника (os error 5). Отсюда: сначала закрыть Claude Code, потом собирать.

  Ручной эквивалент:
    cargo build                                            # loom.exe + loom-mcp.exe
    cargo build --package loom-cli                         # только loom.exe (MCP loom не мешает)
    cargo xtask plugin --project <кит> --engine <движок>   # ещё и C++ плагин

  Держи этот файл в UTF-8 с BOM: Windows PowerShell 5.1 без BOM читает .ps1 как cp1251,
  и типографские тире в русских текстах превращаются в «умные кавычки», ломая разбор.

.PARAMETER Kit
  Каталог кита. По умолчанию $env:LOOM_KIT, иначе известный путь на этой машине.

.PARAMETER Engine
  Каталог движка, нужен только для -Cpp. По умолчанию $env:LOOM_ENGINE, иначе известный путь.

.PARAMETER Release
  Собрать release-профиль и переставить симлинки кита на target\release.

.PARAMETER SkipMcp
  Не собирать loom-mcp.exe — можно при открытом Claude Code (MCP loom останется прежним).

.PARAMETER Check
  Только cargo check: типы проверяются, бинарники не пишутся, клиент можно не закрывать.

.PARAMETER Cpp
  Дополнительно пересобрать C++ плагин: cargo xtask plugin (редактор должен быть закрыт).
  Плагин собирается в debug, поэтому с -Release несовместим.

.PARAMETER StopMcp
  Остановить процессы loom-mcp, чтобы освободить файл. MCP loom в открытых сессиях
  отвалится до перезапуска Claude Code.

.PARAMETER NoRelink
  Не трогать симлинки кита.

.PARAMETER NoSmoke
  Не прогонять loom check по проекту в конце.

.EXAMPLE
  .\rebuild.ps1 -Check
  .\rebuild.ps1 -SkipMcp
  .\rebuild.ps1 -StopMcp
  .\rebuild.ps1 -Cpp
  .\rebuild.ps1 -Release
#>
[CmdletBinding()]
param(
	[string]$Kit = $env:LOOM_KIT,
	[string]$Engine = $env:LOOM_ENGINE,
	[switch]$Release,
	[switch]$SkipMcp,
	[switch]$Check,
	[switch]$Cpp,
	[switch]$StopMcp,
	[switch]$NoRelink,
	[switch]$NoSmoke
)

$ErrorActionPreference = 'Stop'

if (-not $Kit) { $Kit = 'D:\Whiskerwood_IO\WhiskerWood_Mods_LOOM\Whiskerwood-Project' }
if (-not $Engine) { $Engine = 'D:\Whiskerwood_IO\WhiskerWood_Mods_LOOM' }

$repo = $PSScriptRoot
if (-not (Test-Path -LiteralPath (Join-Path $repo 'Cargo.toml'))) {
	throw "$repo не похож на репозиторий Loom: в нём нет Cargo.toml"
}
if ($Cpp -and $Release) { throw '-Cpp и -Release несовместимы: xtask собирает плагин в debug' }
if ($Cpp -and $Check) { throw '-Cpp и -Check несовместимы: check ничего не собирает' }
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'cargo не найден в PATH' }

$buildProfile = if ($Release) { 'release' } else { 'debug' }
$targetDir = Join-Path $repo "target\$buildProfile"
$loomExe = Join-Path $targetDir 'loom.exe'
$mcpExe = Join-Path $targetDir 'loom-mcp.exe'
$partial = $false

function Write-Head([string]$Text) {
	Write-Host ''
	Write-Host "== $Text" -ForegroundColor Cyan
}

function Test-FileLocked([string]$Path) {
	if (-not (Test-Path -LiteralPath $Path)) { return $false }
	try {
		$stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::ReadWrite, [System.IO.FileShare]::None)
		$stream.Close()
		return $false
	} catch {
		return $true
	}
}

function Get-LoomMcpProcesses {
	@(Get-Process -Name 'loom-mcp' -ErrorAction SilentlyContinue)
}

function Stop-LoomMcp {
	$procs = Get-LoomMcpProcesses
	foreach ($p in $procs) { Write-Host "  останавливаю loom-mcp PID $($p.Id)" -ForegroundColor Yellow }
	if ($procs.Count -gt 0) {
		$procs | Stop-Process -Force -ErrorAction SilentlyContinue
		Start-Sleep -Milliseconds 800
	}
	return $procs.Count
}

function Invoke-Cargo([string[]]$CargoArgs) {
	Write-Host "cargo $($CargoArgs -join ' ')" -ForegroundColor DarkGray
	Push-Location $repo
	try { & cargo @CargoArgs } finally { Pop-Location }
	return $LASTEXITCODE
}

# New-Item -ItemType SymbolicLink в Windows PowerShell 5.1 требует прав администратора даже при
# включённом Developer Mode, а CreateSymbolicLink с флагом ALLOW_UNPRIVILEGED_CREATE — нет:
# тем же способом симлинки делает сам xtask (std::os::windows::fs::symlink_dir).
function New-UnprivilegedSymlink([string]$Link, [string]$Target) {
	if (-not ('Loom.Native' -as [type])) {
		Add-Type -Namespace Loom -Name Native -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("kernel32.dll", SetLastError = true, CharSet = System.Runtime.InteropServices.CharSet.Unicode)]
public static extern bool CreateSymbolicLink(string lpSymlinkFileName, string lpTargetFileName, uint dwFlags);
'@
	}
	# 0x2 = ALLOW_UNPRIVILEGED_CREATE, 0x1 = каталог (здесь всегда файл)
	if (-not [Loom.Native]::CreateSymbolicLink($Link, $Target, [uint32]0x2)) {
		$err = [System.Runtime.InteropServices.Marshal]::GetLastWin32Error()
		throw "не удалось создать симлинк $Link -> $Target (Win32 error $err): включи Developer Mode или запусти от администратора"
	}
}

# Переставляет симлинк, но только если на его месте симлинк же: чужой файл не трогаем.
function Set-LoomLink([string]$LinkPath, [string]$TargetPath) {
	if (-not (Test-Path -LiteralPath $TargetPath)) {
		Write-Host "  ! $([IO.Path]::GetFileName($LinkPath)): нет $TargetPath" -ForegroundColor Yellow
		return $false
	}
	$parent = Split-Path -Parent $LinkPath
	if (-not (Test-Path -LiteralPath $parent)) { New-Item -ItemType Directory -Path $parent -Force | Out-Null }
	$existing = Get-Item -LiteralPath $LinkPath -Force -ErrorAction SilentlyContinue
	if ($existing) {
		if ($existing.LinkType -ne 'SymbolicLink') {
			Write-Host "  ! $($existing.Name): на месте симлинка настоящий файл, не трогаю" -ForegroundColor Yellow
			return $false
		}
		$current = @($existing.Target)[0]
		if ($current -eq $TargetPath) {
			Write-Host "  = $($existing.Name) -> $TargetPath"
			return $true
		}
		Remove-Item -LiteralPath $LinkPath -Force
	}
	New-UnprivilegedSymlink -Link $LinkPath -Target $TargetPath
	Write-Host "  + $([IO.Path]::GetFileName($LinkPath)) -> $TargetPath" -ForegroundColor Green
	return $true
}

function Update-KitLinks([string]$KitDir, [string]$EngineDir, [string]$Profile, [string]$Repo) {
	Write-Head "Симлинки кита"
	if (-not (Test-Path -LiteralPath $KitDir)) {
		Write-Host "  ! кита нет: $KitDir" -ForegroundColor Yellow
		return
	}
	$plugin = Join-Path $KitDir 'Plugins\LoomEditor'
	if (-not (Test-Path -LiteralPath $plugin)) {
		Write-Host "  ! в ките нет Plugins\LoomEditor — плагин не слинкован" -ForegroundColor Yellow
		Write-Host "    поставь: cargo xtask plugin --project `"$KitDir`" --engine `"$EngineDir`""
		return
	}
	$item = Get-Item -LiteralPath $plugin -Force
	if ($item.LinkType -ne 'SymbolicLink') {
		Write-Host "  ! Plugins\LoomEditor в ките — копия, а не симлинк: правки репозитория в неё не попадут" -ForegroundColor Yellow
		Write-Host "    переделай: cargo xtask plugin --project `"$KitDir`" --engine `"$EngineDir`""
		return
	}
	Write-Host "  китовый плагин -> $($item.Target)"
	$bin = Join-Path $plugin 'Binaries\ThirdParty\Loom\Win64'
	foreach ($name in 'loom.exe', 'loom-mcp.exe') {
		Set-LoomLink -LinkPath (Join-Path $bin $name) -TargetPath (Join-Path $Repo "target\$Profile\$name") | Out-Null
	}
}

function Invoke-Smoke([string]$LoomExe, [string]$KitDir) {
	Write-Head 'Проверка loom.exe'
	if (-not (Test-Path -LiteralPath $KitDir)) {
		Write-Host "  ! кита нет: $KitDir" -ForegroundColor Yellow
		return
	}
	& $LoomExe docs > $null 2>&1
	if ($LASTEXITCODE -ne 0) {
		Write-Host "  ! loom.exe не запускается (exit $LASTEXITCODE)" -ForegroundColor Red
		return
	}
	$json = & $LoomExe check --project $KitDir --json 2>$null
	$code = $LASTEXITCODE
	try {
		$result = ($json -join "`n") | ConvertFrom-Json
	} catch {
		Write-Host "  check: не разобрал JSON (exit $code)" -ForegroundColor Yellow
		return
	}
	if ($result.ok) {
		Write-Host "  check: $($result.sources) исходников, ошибок нет" -ForegroundColor Green
	} else {
		$errors = @($result.errors)
		Write-Host "  check: $($result.sources) исходников, ошибок $($errors.Count)" -ForegroundColor Yellow
		foreach ($e in $errors | Select-Object -First 3) {
			$where = if ($e.file) { "$($e.file):$($e.line):$($e.column)" } else { 'проект' }
			Write-Host "    $where — $($e.message)"
		}
		if ($errors.Count -gt 3) { Write-Host "    ... ещё $($errors.Count - 3)" }
	}
}

function Show-Binaries([string]$Profile) {
	Write-Head "Собрано (target\$Profile)"
	$any = $false
	foreach ($path in (Join-Path $repo "target\$Profile\loom.exe"), (Join-Path $repo "target\$Profile\loom-mcp.exe")) {
		if (Test-Path -LiteralPath $path) {
			$item = Get-Item -LiteralPath $path
			Write-Host ("  {0,-14} {1,6:N1} МБ  {2}" -f $item.Name, ($item.Length / 1MB), $item.LastWriteTime)
			$any = $true
		}
	}
	if (-not $any) { Write-Host '  (пусто)' -ForegroundColor Yellow }
}

Write-Host "Loom: $repo" -ForegroundColor Cyan
Write-Host "кит:  $Kit"
Write-Host "профиль: $buildProfile"

if ($Check) {
	Write-Head 'cargo check (бинарники не пишутся)'
	$code = Invoke-Cargo @('check')
	if ($code -ne 0) { throw "cargo check вышел с кодом $code" }
	Write-Host '  типы в порядке' -ForegroundColor Green
	exit 0
}

$buildCli = $true
$buildMcp = -not $SkipMcp

if ($buildMcp -and (Test-FileLocked $mcpExe)) {
	$holders = Get-LoomMcpProcesses
	Write-Head 'Файл занят'
	Write-Host "  loom-mcp.exe держат (обычно это MCP-сервер loom в открытом Claude Code):" -ForegroundColor Yellow
	foreach ($p in $holders) { Write-Host "    PID $($p.Id)  $($p.Path)" }
	if ($StopMcp) {
		Stop-LoomMcp | Out-Null
		if (Test-FileLocked $mcpExe) {
			Write-Host '  ! файл всё ещё занят кем-то другим' -ForegroundColor Yellow
		} else {
			Write-Host '  файл освобождён' -ForegroundColor Green
		}
	}
	if (Test-FileLocked $mcpExe) {
		$buildMcp = $false
		$partial = $true
		Write-Host '  ! loom-mcp собирать не буду: закрой Claude Code (или запусти с -StopMcp) и повтори,' -ForegroundColor Yellow
		Write-Host '    либо собирай только loom.exe: -SkipMcp' -ForegroundColor Yellow
	}
}

if ($buildCli -and (Test-FileLocked $loomExe)) {
	Write-Host '  ! loom.exe занят (идёт сборка Blueprint-ов?): собирать его не буду' -ForegroundColor Yellow
	$buildCli = $false
	$partial = $true
}

if ($Cpp) {
	Write-Head 'C++ плагин (cargo xtask plugin)'
	$editors = @(Get-Process -Name 'UnrealEditor', 'UnrealEditor-Cmd' -ErrorAction SilentlyContinue)
	if ($editors.Count -gt 0) {
		foreach ($e in $editors) { Write-Host "  запущен $($e.ProcessName) PID $($e.Id)" -ForegroundColor Yellow }
		throw 'закрой редактор: xtask сносит и пересобирает плагин, открытый редактор его держит'
	}
	if (-not (Test-Path -LiteralPath (Join-Path $Engine 'Engine\Binaries\Win64\UnrealEditor-Cmd.exe'))) {
		throw "не похоже на движок: $Engine"
	}
	if (Test-FileLocked $mcpExe) {
		throw 'loom-mcp.exe занят, а xtask сначала собирает Rust: закрой Claude Code (или -StopMcp)'
	}
	$code = Invoke-Cargo @('xtask', 'plugin', '--project', $Kit, '--engine', $Engine)
	if ($code -ne 0) { throw "cargo xtask plugin вышел с кодом $code" }
} elseif ($buildCli -or $buildMcp) {
	Write-Head 'Rust'
	$cargoArgs = @('build')
	if ($Release) { $cargoArgs += '--release' }
	if (-not $buildMcp) { $cargoArgs += @('--package', 'loom-cli') }
	elseif (-not $buildCli) { $cargoArgs += @('--package', 'loom-mcp') }
	$code = Invoke-Cargo $cargoArgs
	if ($code -ne 0) {
		if (Test-FileLocked $mcpExe) {
			throw 'сборка упала на занятом файле: закрой Claude Code (или -StopMcp) и повтори'
		}
		throw "cargo build вышел с кодом $code"
	}
} else {
	Write-Host ''
	Write-Host 'собирать нечего: оба бинарника заняты' -ForegroundColor Yellow
}

if (-not $NoRelink) { Update-KitLinks -KitDir $Kit -EngineDir $Engine -Profile $buildProfile -Repo $repo }
Show-Binaries $buildProfile
if (-not $NoSmoke -and $Cpp) {
	# xtask уже пересобрал плагин: ссылки в ките ведут в out\, проверяем то, что реально в ките.
	Invoke-Smoke -LoomExe (Join-Path $Kit 'Plugins\LoomEditor\Binaries\ThirdParty\Loom\Win64\loom.exe') -KitDir $Kit
} elseif (-not $NoSmoke) {
	Invoke-Smoke -LoomExe $loomExe -KitDir $Kit
}

Write-Head 'Итог'
if ($partial) {
	Write-Host '  частично: loom-mcp.exe не пересобран, MCP loom работает на прежней сборке' -ForegroundColor Yellow
	Write-Host '  закрой Claude Code и запусти скрипт снова' -ForegroundColor Yellow
	exit 2
}
Write-Host '  готово' -ForegroundColor Green
Write-Host '  MCP whiskerwood подхватит новый loom.exe сразу, MCP loom — при следующем запуске Claude Code'
Write-Host '  после правок формата ops/типов прогони сборку Blueprint-ов (ww_loom_build): плагин обновит types.json и ops'
exit 0