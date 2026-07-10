param(
    [string]$TargetExe,

    [string]$ShimDir = (Join-Path $env:LOCALAPPDATA "Programs\CodexPlusPlus\bin"),

    [switch]$Install,
    [switch]$Remove,
    [switch]$AddToUserPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Resolve-FullPath {
    param([string]$Path)
    $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path)
}

function Move-ToPathFront {
    param(
        [string]$PathValue,
        [string]$Entry
    )

    $needle = $Entry.TrimEnd("\")
    $segments = @()
    if (-not [string]::IsNullOrWhiteSpace($PathValue)) {
        foreach ($segment in $PathValue.Split(";", [System.StringSplitOptions]::RemoveEmptyEntries)) {
            if ($segment.TrimEnd("\") -ine $needle) {
                $segments += $segment
            }
        }
    }

    (@($Entry) + $segments) -join ";"
}

$ShimDir = Resolve-FullPath -Path $ShimDir
$shimPath = Join-Path $ShimDir "codex.ps1"

if ($Remove) {
    if (Test-Path -LiteralPath $shimPath -PathType Leaf) {
        Remove-Item -LiteralPath $shimPath
        Write-Host "==> Removed shim at $shimPath"
    } else {
        Write-Host "==> No shim found at $shimPath"
    }
    exit 0
}

if ([string]::IsNullOrWhiteSpace($TargetExe)) {
    Write-Error "-TargetExe is required unless -Remove is set."
    exit 2
}

$targetPath = Resolve-FullPath -Path $TargetExe
$activeCodex = Get-Command codex -ErrorAction SilentlyContinue
$activePath = if ($activeCodex) { $activeCodex.Source } else { "not found on PATH" }

Write-Host "==> Codex++ shim"
Write-Host "Active codex path: $activePath"
Write-Host "Shim path: $shimPath"
Write-Host "Target fork executable: $targetPath"
Write-Host "Target reachable: $(Test-Path -LiteralPath $targetPath -PathType Leaf)"

if (-not $Install) {
    Write-Host "==> Dry run only; pass -Install to write the shim."
    exit 0
}

if (-not (Test-Path -LiteralPath $targetPath -PathType Leaf)) {
    Write-Error "Target fork executable does not exist: $targetPath"
    exit 1
}

New-Item -ItemType Directory -Force -Path $ShimDir | Out-Null
$content = @"
`$target = '$($targetPath.Replace("'", "''"))'
if (`$MyInvocation.ExpectingInput) {
    `$input | & `$target @args
} else {
    & `$target @args
}
if (`$null -ne `$global:LASTEXITCODE) { exit `$global:LASTEXITCODE }
"@
[System.IO.File]::WriteAllText($shimPath, $content, [System.Text.UTF8Encoding]::new($true))
Write-Host "==> Installed shim at $shimPath"

if ($AddToUserPath) {
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $newUserPath = Move-ToPathFront -PathValue $userPath -Entry $ShimDir
    [Environment]::SetEnvironmentVariable("Path", $newUserPath, "User")
    $env:Path = Move-ToPathFront -PathValue $env:Path -Entry $ShimDir
    Write-Host "==> Moved $ShimDir to the front of the user PATH for future shells."
}

$currentCodex = Get-Command codex -ErrorAction SilentlyContinue
if ($currentCodex -and $currentCodex.Source -ieq $shimPath) {
    Write-Host "==> Run: codex"
} else {
    Write-Host "==> Run now: `$env:Path = `"$ShimDir;`$env:Path`"; codex"
}
