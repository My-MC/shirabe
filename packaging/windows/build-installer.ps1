#requires -Version 5.1
<#
.SYNOPSIS
Builds the unsigned Windows x64 installer for Shirabe Meter Suite with Inno
Setup 6.

.DESCRIPTION
Reads the version from [workspace.package] in the workspace Cargo.toml, checks
that the packaged binaries do not import the Visual C++ runtime, compiles
packaging/windows/shirabe.iss and prints the setup path and SHA-256.

.PARAMETER Dist
Directory with the VST3 bundle, CLAP and shirabe.exe (default
dist/shirabe-windows-x64).

.PARAMETER OutDir
Directory for the setup executable (default dist/installers).

.PARAMETER Version
Override the version from Cargo.toml.

.PARAMETER Iscc
Path to ISCC.exe. By default the Inno Setup 6 installation and PATH are tried.

.EXAMPLE
.\packaging\windows\build-installer.ps1
#>
[CmdletBinding()]
param(
    [string]$Dist = "dist/shirabe-windows-x64",
    [string]$OutDir = "dist/installers",
    [string]$Version,
    [string]$Iscc
)

$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$product = "Shirabe Meter Suite"

function Resolve-RepoPath {
    param([string]$Path)
    if ([IO.Path]::IsPathRooted($Path)) {
        return $Path
    }
    return (Join-Path $repoRoot $Path)
}

function Get-WorkspaceVersion {
    $cargoToml = Join-Path $repoRoot "Cargo.toml"
    if (-not (Test-Path $cargoToml)) {
        throw "Cargo.toml not found at $cargoToml"
    }
    $text = Get-Content -Raw -LiteralPath $cargoToml
    $section = [regex]::Match($text, '(?ms)^\[workspace\.package\]\s*(.*?)(?=^\[|\z)')
    if (-not $section.Success) {
        throw "[workspace.package] not found in $cargoToml"
    }
    $match = [regex]::Match($section.Groups[1].Value, '(?m)^\s*version\s*=\s*"([^"]+)"')
    if (-not $match.Success) {
        throw "version not found in [workspace.package]"
    }
    return $match.Groups[1].Value
}

function Assert-StaticCrt {
    param([string]$Path)
    $text = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($Path))
    if ($text -match '(?i)(vcruntime|msvcp)\d+[_a-z0-9]*\.dll') {
        throw "$Path imports $($Matches[0]); the installer must only contain statically linked binaries"
    }
    Write-Host "static CRT OK: $Path"
}

function Find-Iscc {
    param([string]$Explicit)
    if ($Explicit) {
        if (-not (Test-Path $Explicit)) {
            throw "ISCC.exe not found at $Explicit"
        }
        return (Resolve-Path $Explicit).Path
    }
    $candidates = @()
    if (${env:ProgramFiles(x86)}) {
        $candidates += Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"
    }
    if ($env:ProgramFiles) {
        $candidates += Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe"
    }
    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) {
            return $candidate
        }
    }
    $command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }
    throw "ISCC.exe (Inno Setup 6) not found. Install Inno Setup 6 or pass -Iscc."
}

if (-not $Version) {
    $Version = Get-WorkspaceVersion
}
if ($Version -notmatch '^\d+\.\d+\.\d+([-+][0-9A-Za-z.-]+)?$') {
    throw "unexpected version: $Version"
}

$distPath = Resolve-RepoPath $Dist
$outPath = Resolve-RepoPath $OutDir

$vst3Binary = Join-Path $distPath "$product.vst3\Contents\x86_64-win\$product.vst3"
$clapFile = Join-Path $distPath "$product.clap"
$exeFile = Join-Path $distPath "shirabe.exe"

foreach ($required in @($vst3Binary, $clapFile, $exeFile)) {
    if (-not (Test-Path $required)) {
        throw "missing $required; run cargo xtask bundle shirabe-plugin --release and cargo build --release -p shirabe-app --bin shirabe first"
    }
}

Assert-StaticCrt $vst3Binary
Assert-StaticCrt $clapFile
Assert-StaticCrt $exeFile

$iscc = Find-Iscc $Iscc
New-Item -ItemType Directory -Force -Path $outPath | Out-Null

$outputBaseName = "ShirabeMeterSuite-$Version-windows-x64-setup"
$arguments = @(
    "/DAppVersion=$Version",
    "/DDistDir=$distPath",
    "/DOutputDir=$outPath",
    "/DOutputBaseFilename=$outputBaseName",
    "/Qp",
    (Join-Path $PSScriptRoot "shirabe.iss")
)

Write-Host "ISCC: $iscc"
& $iscc @arguments
if ($LASTEXITCODE -ne 0) {
    throw "ISCC failed with exit code $LASTEXITCODE"
}

$setup = Join-Path $outPath "$outputBaseName.exe"
if (-not (Test-Path $setup)) {
    throw "ISCC reported success but $setup does not exist"
}

$versionInfo = (Get-Item -LiteralPath $setup).VersionInfo
if ($versionInfo.ProductVersion -and $versionInfo.ProductVersion -notlike "$Version*") {
    throw "installer product version $($versionInfo.ProductVersion) does not match $Version"
}

$signature = Get-AuthenticodeSignature -LiteralPath $setup
Write-Host "signature: $($signature.Status) (unsigned builds are expected)"

$hash = (Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash.ToLowerInvariant()
$size = (Get-Item -LiteralPath $setup).Length
Write-Host "Built $setup"
Write-Host "Version $Version, size $size bytes, sha256 $hash"
Write-Output $setup
