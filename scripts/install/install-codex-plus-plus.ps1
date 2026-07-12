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
$cmdShimPath = Join-Path $ShimDir "codex.cmd"
$targetLinkPath = Join-Path $ShimDir ".codex-plus-plus-target"
$markerPath = Join-Path $ShimDir ".codex-plus-plus-shim"

if ($Remove) {
    if (Test-Path -LiteralPath $shimPath -PathType Leaf) {
        Remove-Item -LiteralPath $shimPath
        Write-Host "==> Removed shim at $shimPath"
    } else {
        Write-Host "==> No shim found at $shimPath"
    }
    $ownsCompanion = (Test-Path -LiteralPath $cmdShimPath -PathType Leaf) -and
        (Test-Path -LiteralPath $markerPath -PathType Leaf) -and
        (Get-Content -LiteralPath $markerPath -Raw).Trim() -ceq (Get-FileHash -LiteralPath $cmdShimPath -Algorithm SHA256).Hash
    if ($ownsCompanion) {
        if (Test-Path -LiteralPath $cmdShimPath -PathType Leaf) {
            Remove-Item -LiteralPath $cmdShimPath
            Write-Host "==> Removed shim at $cmdShimPath"
        }
        $existingTargetLink = Get-Item -LiteralPath $targetLinkPath -Force -ErrorAction SilentlyContinue
        if ($existingTargetLink -and $existingTargetLink.LinkType -eq "Junction") {
            $existingTargetLink.Delete()
        }
        Remove-Item -LiteralPath $markerPath
    } elseif (Test-Path -LiteralPath $cmdShimPath -PathType Leaf) {
        Write-Host "==> Kept non-shim launcher at $cmdShimPath"
    } else {
        Write-Host "==> No shim found at $cmdShimPath"
    }
    exit 0
}

if ([string]::IsNullOrWhiteSpace($TargetExe)) {
    Write-Error "-TargetExe is required unless -Remove is set."
    exit 2
}

$targetPath = Resolve-FullPath -Path $TargetExe
if ($targetPath.StartsWith("\\")) {
    Write-Error "Target fork executable must be on a local filesystem: $targetPath"
    exit 1
}
$activeCodex = Get-Command codex -ErrorAction SilentlyContinue
$activePath = if ($activeCodex) { $activeCodex.Source } else { "not found on PATH" }

Write-Host "==> Codex++ shim"
Write-Host "Active codex path: $activePath"
Write-Host "Shim paths: $shimPath, $cmdShimPath"
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
$targetFileName = [System.IO.Path]::GetFileName($targetPath)
if ($targetFileName -ine "codex.exe") {
    Write-Error "Target fork executable must be named codex.exe: $targetPath"
    exit 1
}
$cmdContent = @"
@echo off
setlocal DisableDelayedExpansion
"%~dp0.codex-plus-plus-target\$targetFileName" %*
exit /b %ERRORLEVEL%
"@
$existingMarker = Get-Item -LiteralPath $markerPath -Force -ErrorAction SilentlyContinue
$ownsCompanion = $existingMarker -and
    (Test-Path -LiteralPath $cmdShimPath -PathType Leaf) -and
    (Get-Content -LiteralPath $markerPath -Raw).Trim() -ceq (Get-FileHash -LiteralPath $cmdShimPath -Algorithm SHA256).Hash
if ($existingMarker -and -not $ownsCompanion) {
    Write-Error "Refusing to replace unmanaged marker: $markerPath"
    exit 1
}
if ((Test-Path -LiteralPath $cmdShimPath -PathType Leaf) -and -not $ownsCompanion) {
    Write-Error "Refusing to replace non-shim launcher: $cmdShimPath"
    exit 1
}
$existingTargetLink = Get-Item -LiteralPath $targetLinkPath -Force -ErrorAction SilentlyContinue
if ($existingTargetLink -and (-not $ownsCompanion -or $existingTargetLink.LinkType -ne "Junction")) {
    Write-Error "Refusing to replace unmanaged target path: $targetLinkPath"
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
if ($existingTargetLink) {
    $existingTargetLink.Delete()
}
New-Item -ItemType Junction -Path $targetLinkPath -Target (Split-Path -Parent $targetPath) | Out-Null
[System.IO.File]::WriteAllText($cmdShimPath, $cmdContent, [System.Text.UTF8Encoding]::new($false))
[System.IO.File]::WriteAllText($markerPath, (Get-FileHash -LiteralPath $cmdShimPath -Algorithm SHA256).Hash, [System.Text.UTF8Encoding]::new($false))
Write-Host "==> Installed shims at $shimPath and $cmdShimPath"

if ($AddToUserPath) {
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $newUserPath = Move-ToPathFront -PathValue $userPath -Entry $ShimDir
    [Environment]::SetEnvironmentVariable("Path", $newUserPath, "User")
    $env:Path = Move-ToPathFront -PathValue $env:Path -Entry $ShimDir
    Write-Host "==> Moved $ShimDir to the front of the user PATH for future shells."
}

$currentCodex = Get-Command codex -ErrorAction SilentlyContinue
if ($currentCodex -and @($shimPath, $cmdShimPath) -icontains $currentCodex.Source) {
    Write-Host "==> Run: codex"
} else {
    Write-Host "==> Run now: `$env:Path = `"$ShimDir;`$env:Path`"; codex"
}
