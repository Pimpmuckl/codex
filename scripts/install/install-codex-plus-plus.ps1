param(
    [Parameter(Mandatory = $true)]
    [string]$TargetExe,

    [string]$ShimDir = (Join-Path $env:LOCALAPPDATA "Programs\CodexPlusPlus\bin"),

    [switch]$Install,
    [switch]$AddToUserPath,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Write-Step {
    param([string]$Message)
    Write-Host "==> $Message"
}

function Resolve-FullPath {
    param([string]$Path)

    if ([string]::IsNullOrWhiteSpace($Path)) {
        return $Path
    }

    return $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path)
}

function Get-NpmCodexVersion {
    $npm = Get-Command npm.cmd -ErrorAction SilentlyContinue
    if (-not $npm) {
        $npm = Get-Command npm.exe -ErrorAction SilentlyContinue
    }
    if (-not $npm) {
        $npm = Get-Command npm -ErrorAction SilentlyContinue
    }
    if (-not $npm) {
        return "npm not found"
    }

    try {
        $json = & $npm.Source list -g @openai/codex --depth=0 --json 2>$null
    } catch {
        return "not discoverable"
    }

    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($json)) {
        return "not installed or not discoverable"
    }

    try {
        $parsed = $json | ConvertFrom-Json
        $version = $parsed.dependencies.'@openai/codex'.version
        if ([string]::IsNullOrWhiteSpace($version)) {
            return "not installed or not discoverable"
        }

        return $version
    } catch {
        return "not discoverable"
    }
}

function Get-FutureExecutionPolicy {
    foreach ($scope in @("MachinePolicy", "UserPolicy", "CurrentUser", "LocalMachine")) {
        $policy = Get-ExecutionPolicy -Scope $scope
        if ($policy -ne "Undefined") {
            return "$scope=$policy"
        }
    }

    return "Default=Restricted"
}

function Write-ExecutionPolicyWarning {
    param([string]$Policy)

    if ($Policy.EndsWith("=Restricted") -or $Policy.EndsWith("=AllSigned")) {
        Write-Warning "Future PowerShell sessions may block codex.ps1 under $Policy. Use a less restrictive policy for this trusted local shim or choose another shim directory/launcher."
    }
}

function Path-Contains {
    param(
        [string]$PathValue,
        [string]$Entry
    )

    if ([string]::IsNullOrWhiteSpace($PathValue)) {
        return $false
    }

    $needle = $Entry.TrimEnd("\")
    foreach ($segment in $PathValue.Split(";", [System.StringSplitOptions]::RemoveEmptyEntries)) {
        if ($segment.TrimEnd("\") -ieq $needle) {
            return $true
        }
    }

    return $false
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

    return (@($Entry) + $segments) -join ";"
}

function Write-Shim {
    param(
        [string]$PsShimPath,
        [string]$ForkExe
    )

    $psContent = @"
`$target = '$($ForkExe.Replace("'", "''"))'
if (`$MyInvocation.ExpectingInput) {
    `$input | & `$target @args
} else {
    & `$target @args
}
if (`$null -ne `$global:LASTEXITCODE) { exit `$global:LASTEXITCODE }
"@
    [System.IO.File]::WriteAllText($PsShimPath, $psContent, [System.Text.UTF8Encoding]::new($true))
}

function Test-CommandIsShim {
    param(
        [object]$Command,
        [string]$ExpectedShimPath
    )

    if (-not $Command) {
        return $false
    }

    return $Command.Source -ieq $ExpectedShimPath
}

function Test-ShimWins {
    param(
        [string]$ExpectedShimPath,
        [string]$UserPath
    )

    $originalPath = $env:Path
    try {
        $machinePath = [Environment]::GetEnvironmentVariable("Path", "Machine")
        if ([string]::IsNullOrWhiteSpace($machinePath)) {
            $env:Path = $UserPath
        } else {
            $env:Path = "$machinePath;$UserPath"
        }

        $winner = Get-Command codex -ErrorAction SilentlyContinue
        return Test-CommandIsShim -Command $winner -ExpectedShimPath $ExpectedShimPath
    } finally {
        $env:Path = $originalPath
    }
}

function Test-ShimWinsWithPath {
    param(
        [string]$ExpectedShimPath,
        [string]$PathValue
    )

    $originalPath = $env:Path
    try {
        $env:Path = $PathValue
        $winner = Get-Command codex -ErrorAction SilentlyContinue
        return Test-CommandIsShim -Command $winner -ExpectedShimPath $ExpectedShimPath
    } finally {
        $env:Path = $originalPath
    }
}

function Test-CurrentShimWins {
    param([string]$ExpectedShimPath)

    $winner = Get-Command codex -ErrorAction SilentlyContinue
    return Test-CommandIsShim -Command $winner -ExpectedShimPath $ExpectedShimPath
}

$targetPath = Resolve-FullPath -Path $TargetExe
$ShimDir = Resolve-FullPath -Path $ShimDir
$psShimPath = Join-Path $ShimDir "codex.ps1"
$activeCodex = Get-Command codex -ErrorAction SilentlyContinue
$activePath = if ($activeCodex) { $activeCodex.Source } else { "not found on PATH" }
$targetReachable = Test-Path -LiteralPath $targetPath -PathType Leaf
$npmVersion = Get-NpmCodexVersion
$futureExecutionPolicy = Get-FutureExecutionPolicy

Write-Step "Codex++ shim self-check"
Write-Host "Active codex path: $activePath"
Write-Host "Shim path: $psShimPath"
Write-Host "Target fork executable: $targetPath"
Write-Host "Target reachable: $targetReachable"
Write-Host "Global npm @openai/codex version: $npmVersion"
Write-Host "Future PowerShell execution policy: $futureExecutionPolicy"
Write-ExecutionPolicyWarning -Policy $futureExecutionPolicy

if ($DryRun -or -not $Install) {
    Write-Step "Dry run only; no files or PATH entries changed."
    exit 0
}

if (-not $targetReachable) {
    Write-Error "Target fork executable does not exist: $targetPath"
    exit 1
}

New-Item -ItemType Directory -Force -Path $ShimDir | Out-Null
Write-Shim -PsShimPath $psShimPath -ForkExe $targetPath
Write-Step "Installed PowerShell shim at $psShimPath"

if ($AddToUserPath) {
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $newUserPath = Move-ToPathFront -PathValue $userPath -Entry $ShimDir
    [Environment]::SetEnvironmentVariable("Path", $newUserPath, "User")
    $env:Path = Move-ToPathFront -PathValue $env:Path -Entry $ShimDir
    Write-Step "Moved $ShimDir to the front of the user PATH for future shells."

    if (-not (Test-ShimWins -ExpectedShimPath $psShimPath -UserPath $newUserPath)) {
        Write-Error "The shim is still shadowed by another codex command. Use a machine-priority shim directory or remove the earlier or stale Codex entry."
        exit 1
    }
}

if (Test-CurrentShimWins -ExpectedShimPath $psShimPath) {
    Write-Step "Run: codex"
} elseif (Test-ShimWinsWithPath -ExpectedShimPath $psShimPath -PathValue (Move-ToPathFront -PathValue $env:Path -Entry $ShimDir)) {
    Write-Step ('Run now: $env:Path = "{0};$env:Path"; codex' -f $ShimDir)
} else {
    Write-Warning "Another codex command shadows $psShimPath even when $ShimDir is first on PATH."
    Write-Step ('Run shim directly: & "{0}"' -f $psShimPath)
}
