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

function Get-Sha256 {
    param([string]$Path)

    $stream = [System.IO.File]::OpenRead($Path)
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
        return ([System.BitConverter]::ToString($sha256.ComputeHash($stream))).Replace("-", "")
    } finally {
        $sha256.Dispose()
        $stream.Dispose()
    }
}

function Test-IsJunction {
    param([object]$Item)

    return $null -ne $Item -and
        ($Item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -and
        $Item.LinkType -eq "Junction"
}

function Test-ReleaseFilesUnlocked {
    param([string]$ReleaseDir)

    try {
        $files = @(Get-ChildItem -LiteralPath $ReleaseDir -File -Recurse -Force -ErrorAction Stop)
    } catch {
        return $false
    }
    foreach ($file in $files) {
        $stream = $null
        try {
            $stream = [System.IO.File]::Open(
                $file.FullName,
                [System.IO.FileMode]::Open,
                [System.IO.FileAccess]::Read,
                [System.IO.FileShare]::None
            )
        } catch {
            return $false
        } finally {
            if ($null -ne $stream) {
                $stream.Dispose()
            }
        }
    }
    return $true
}

function Remove-StaleReleases {
    param(
        [string]$ReleasesDir,
        [string]$KeepReleaseDir
    )

    if (-not (Test-Path -LiteralPath $ReleasesDir -PathType Container)) {
        return
    }
    foreach ($release in Get-ChildItem -LiteralPath $ReleasesDir -Directory -Force -ErrorAction SilentlyContinue) {
        if ($release.Name.StartsWith(".staging.")) {
            Remove-Item -LiteralPath $release.FullName -Recurse -Force -ErrorAction SilentlyContinue
            continue
        }
        if (-not [string]::IsNullOrWhiteSpace($KeepReleaseDir) -and
            $release.FullName.Equals($KeepReleaseDir, [System.StringComparison]::OrdinalIgnoreCase)) {
            continue
        }
        if (-not (Test-ReleaseFilesUnlocked -ReleaseDir $release.FullName)) {
            Write-Host "==> Kept active Codex++ release at $($release.FullName)"
            continue
        }
        try {
            Remove-Item -LiteralPath $release.FullName -Recurse -Force
            Write-Host "==> Removed stale Codex++ release at $($release.FullName)"
        } catch {
            Write-Warning "Could not remove stale Codex++ release at $($release.FullName): $($_.Exception.Message)"
        }
    }
}

$ShimDir = Resolve-FullPath -Path $ShimDir
$shimPath = Join-Path $ShimDir "codex.ps1"
$cmdShimPath = Join-Path $ShimDir "codex.cmd"
$targetPointerPath = Join-Path $ShimDir ".codex-plus-plus-target"
$markerPath = Join-Path $ShimDir ".codex-plus-plus-shim"
$codexHome = if ([string]::IsNullOrWhiteSpace($env:CODEX_HOME)) {
    Join-Path $env:USERPROFILE ".codex"
} else {
    Resolve-FullPath -Path $env:CODEX_HOME
}
$releasesDir = Join-Path $codexHome "packages\codex-plus-plus\releases"

if ($Remove) {
    if (Test-Path -LiteralPath $shimPath -PathType Leaf) {
        Remove-Item -LiteralPath $shimPath
        Write-Host "==> Removed shim at $shimPath"
    } else {
        Write-Host "==> No shim found at $shimPath"
    }
    $ownsCompanion = (Test-Path -LiteralPath $cmdShimPath -PathType Leaf) -and
        (Test-Path -LiteralPath $markerPath -PathType Leaf) -and
        (Get-Content -LiteralPath $markerPath -Raw).Trim() -ceq (Get-Sha256 -Path $cmdShimPath)
    if ($ownsCompanion) {
        if (Test-Path -LiteralPath $cmdShimPath -PathType Leaf) {
            Remove-Item -LiteralPath $cmdShimPath
            Write-Host "==> Removed shim at $cmdShimPath"
        }
        $existingTarget = Get-Item -LiteralPath $targetPointerPath -Force -ErrorAction SilentlyContinue
        if (Test-IsJunction -Item $existingTarget) {
            $existingTarget.Delete()
        } elseif ($existingTarget -and -not $existingTarget.PSIsContainer) {
            Remove-Item -LiteralPath $targetPointerPath
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
$targetBinDir = Split-Path -Parent $targetPath
if ((Split-Path -Leaf $targetBinDir) -ine "bin") {
    Write-Error "Target fork executable must be inside a package bin directory: $targetPath"
    exit 1
}
$packageDir = Split-Path -Parent $targetBinDir
if (-not (Test-Path -LiteralPath (Join-Path $packageDir "codex-package.json") -PathType Leaf)) {
    Write-Error "Target fork executable must belong to a Codex package: $targetPath"
    exit 1
}
$packageDirPrefix = $packageDir.TrimEnd("\") + "\"
if ($releasesDir.Equals($packageDir, [System.StringComparison]::OrdinalIgnoreCase) -or
    $releasesDir.StartsWith($packageDirPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    Write-Error "Codex++ managed releases must be outside the source package: $packageDir"
    exit 1
}
$cmdContent = @"
@echo off
setlocal DisableDelayedExpansion
set /p "CODEX_PLUS_PLUS_TARGET="<"%~dp0.codex-plus-plus-target"
if not defined CODEX_PLUS_PLUS_TARGET exit /b 1
"%CODEX_PLUS_PLUS_TARGET%\$targetFileName" %*
exit /b %ERRORLEVEL%
"@
$existingMarker = Get-Item -LiteralPath $markerPath -Force -ErrorAction SilentlyContinue
$ownsCompanion = $existingMarker -and
    (Test-Path -LiteralPath $cmdShimPath -PathType Leaf) -and
    (Get-Content -LiteralPath $markerPath -Raw).Trim() -ceq (Get-Sha256 -Path $cmdShimPath)
if ($existingMarker -and -not $ownsCompanion) {
    Write-Error "Refusing to replace unmanaged marker: $markerPath"
    exit 1
}
if ((Test-Path -LiteralPath $cmdShimPath -PathType Leaf) -and -not $ownsCompanion) {
    Write-Error "Refusing to replace non-shim launcher: $cmdShimPath"
    exit 1
}
$existingTarget = Get-Item -LiteralPath $targetPointerPath -Force -ErrorAction SilentlyContinue
$managedTarget = $existingTarget -and
    ((Test-IsJunction -Item $existingTarget) -or -not $existingTarget.PSIsContainer)
if ($existingTarget -and (-not $ownsCompanion -or -not $managedTarget)) {
    Write-Error "Refusing to replace unmanaged target path: $targetPointerPath"
    exit 1
}

New-Item -ItemType Directory -Force -Path $ShimDir | Out-Null
New-Item -ItemType Directory -Force -Path $releasesDir | Out-Null
$releaseName = [DateTime]::UtcNow.ToString(
    "yyyyMMddTHHmmssfffffffZ",
    [System.Globalization.CultureInfo]::InvariantCulture
)
$releaseDir = Join-Path $releasesDir $releaseName
$stagingDir = Join-Path $releasesDir ".staging.$releaseName.$PID"
try {
    New-Item -ItemType Directory -Path $stagingDir | Out-Null
    Get-ChildItem -LiteralPath $packageDir -Force |
        Copy-Item -Destination $stagingDir -Recurse -Force
    Move-Item -LiteralPath $stagingDir -Destination $releaseDir
} finally {
    if (Test-Path -LiteralPath $stagingDir) {
        Remove-Item -LiteralPath $stagingDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
$installedBinDir = Join-Path $releaseDir "bin"
$installedTargetPath = Join-Path $installedBinDir $targetFileName
$content = @"
`$targetDir = [System.IO.File]::ReadAllText((Join-Path `$PSScriptRoot '.codex-plus-plus-target')).Trim()
`$target = Join-Path `$targetDir '$targetFileName'
if (`$MyInvocation.ExpectingInput) {
    `$input | & `$target @args
} else {
    & `$target @args
}
if (`$null -ne `$global:LASTEXITCODE) { exit `$global:LASTEXITCODE }
"@
[System.IO.File]::WriteAllText($shimPath, $content, [System.Text.UTF8Encoding]::new($true))
[System.IO.File]::WriteAllText($cmdShimPath, $cmdContent, [System.Text.UTF8Encoding]::new($false))
[System.IO.File]::WriteAllText($markerPath, (Get-Sha256 -Path $cmdShimPath), [System.Text.UTF8Encoding]::new($false))
if (-not (Test-Path -LiteralPath $installedTargetPath -PathType Leaf)) {
    Write-Error "Installed Codex++ target is not reachable: $installedTargetPath"
    exit 1
}
$targetPointerTempPath = "$targetPointerPath.$PID.tmp"
$targetPointerBackupPath = "$targetPointerPath.$PID.bak"
try {
    [System.IO.File]::WriteAllText(
        $targetPointerTempPath,
        $installedBinDir,
        [System.Text.Encoding]::Default
    )
    if (Test-IsJunction -Item $existingTarget) {
        $existingTarget.Delete()
        Move-Item -LiteralPath $targetPointerTempPath -Destination $targetPointerPath
    } elseif ($existingTarget) {
        [System.IO.File]::Replace(
            $targetPointerTempPath,
            $targetPointerPath,
            $targetPointerBackupPath
        )
    } else {
        Move-Item -LiteralPath $targetPointerTempPath -Destination $targetPointerPath
    }
} finally {
    Remove-Item -LiteralPath $targetPointerTempPath -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $targetPointerBackupPath -Force -ErrorAction SilentlyContinue
}
Write-Host "==> Installed shims at $shimPath and $cmdShimPath"
Write-Host "==> Active release: $releaseDir"
Remove-StaleReleases -ReleasesDir $releasesDir -KeepReleaseDir $releaseDir

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
