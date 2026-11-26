#requires -Version 5.1
<#
.SYNOPSIS
Installs the Shirabe Meter Suite Windows installer silently, checks the
installed files, then uninstalls it and checks that the files are gone.

.DESCRIPTION
Destructive: it installs into %CommonProgramFiles% and %ProgramFiles% and then
removes those files. Intended for a disposable CI runner or a clean test VM.
The script refuses to start when the product is already installed unless
-Force is given.

.PARAMETER Installer
Path to the setup executable built by build-installer.ps1.

.PARAMETER Dist
Directory with the expected source files for the hash comparison
(default dist/shirabe-windows-x64).

.PARAMETER KeepInstalled
Check the installed files but skip the uninstall step.

.PARAMETER Force
Proceed even when the product already appears to be installed.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Installer,
    [string]$Dist = "dist/shirabe-windows-x64",
    [switch]$KeepInstalled,
    [switch]$Force
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

function Assert-Installed([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) {
        throw "expected after install: $Path"
    }
    Write-Host "found $Path"
}

function Assert-Missing([string]$Path) {
    if (Test-Path -LiteralPath $Path) {
        throw "still present after uninstall: $Path"
    }
    Write-Host "removed $Path"
}

function Wait-Missing([string]$Path, [int]$TimeoutSeconds = 60) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Test-Path -LiteralPath $Path) -and (Get-Date) -lt $deadline) {
        Start-Sleep -Seconds 1
    }
}

$setup = (Resolve-Path $Installer).Path
$distPath = Resolve-RepoPath $Dist

$programFiles = if ($env:ProgramW6432) { $env:ProgramW6432 } else { $env:ProgramFiles }
$commonFiles = if ($env:CommonProgramW6432) { $env:CommonProgramW6432 } else { $env:CommonProgramFiles }

$vst3Dir = Join-Path $commonFiles "VST3\$product.vst3"
$vst3Binary = Join-Path $vst3Dir "Contents\x86_64-win\$product.vst3"
$clapFile = Join-Path $commonFiles "CLAP\$product.clap"
$appDir = Join-Path $programFiles $product
$exeFile = Join-Path $appDir "shirabe.exe"
$startMenu = Join-Path $env:ProgramData "Microsoft\Windows\Start Menu\Programs\$product\$product.lnk"

$alreadyInstalled = @($vst3Dir, $clapFile, $appDir) | Where-Object { Test-Path -LiteralPath $_ }
if ($alreadyInstalled -and -not $Force) {
    throw "the product is already installed ($($alreadyInstalled -join ', ')); pass -Force on a disposable machine"
}

$signature = Get-AuthenticodeSignature -LiteralPath $setup
Write-Host "signature: $($signature.Status) (unsigned builds are expected)"

$installLog = Join-Path $env:TEMP "shirabe-installer-test-install.log"
Write-Host "== silent install =="
$install = Start-Process -FilePath $setup `
    -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', "/LOG=$installLog" `
    -Wait -PassThru
if ($install.ExitCode -ne 0) {
    throw "installer exited with $($install.ExitCode); see $installLog"
}

Assert-Installed $vst3Binary
Assert-Installed $clapFile
Assert-Installed $exeFile
Assert-Installed $startMenu

$licenses = Join-Path $appDir "licenses\GPL-3.0-or-later.txt"
Assert-Installed $licenses

$uninstaller = Get-ChildItem -LiteralPath $appDir -Filter "unins*.exe" | Select-Object -First 1
if (-not $uninstaller) {
    throw "no uninstaller in $appDir"
}
Write-Host "uninstaller: $($uninstaller.FullName)"

$expected = @(
    @{ Installed = $vst3Binary; Source = Join-Path $distPath "$product.vst3\Contents\x86_64-win\$product.vst3" },
    @{ Installed = $clapFile; Source = Join-Path $distPath "$product.clap" },
    @{ Installed = $exeFile; Source = Join-Path $distPath "shirabe.exe" }
)
foreach ($pair in $expected) {
    $installedHash = (Get-FileHash -LiteralPath $pair.Installed -Algorithm SHA256).Hash
    $sourceHash = (Get-FileHash -LiteralPath $pair.Source -Algorithm SHA256).Hash
    if ($installedHash -ne $sourceHash) {
        throw "content mismatch after install: $($pair.Installed)"
    }
    Write-Host "content matches source: $($pair.Installed)"
}

$uninstallKeys = @(
    'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
)
$entry = @(
    foreach ($key in $uninstallKeys) {
        if (Test-Path $key) {
            Get-ChildItem $key -ErrorAction SilentlyContinue |
                ForEach-Object { Get-ItemProperty $_.PSPath } |
                Where-Object { $_.DisplayName -eq $product }
        }
    }
)
if (-not $entry) {
    throw "no uninstall entry with DisplayName '$product'"
}
Write-Host "uninstall entry: $($entry[0].DisplayVersion)"

if ($KeepInstalled) {
    Write-Host "PASS: install checked; left installed (-KeepInstalled)"
    return
}

Write-Host "== silent uninstall =="
$uninstall = Start-Process -FilePath $uninstaller.FullName `
    -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' `
    -Wait -PassThru
if ($uninstall.ExitCode -ne 0) {
    throw "uninstaller exited with $($uninstall.ExitCode)"
}

Wait-Missing $vst3Dir
Wait-Missing $appDir

Assert-Missing $vst3Dir
Assert-Missing $clapFile
Assert-Missing $appDir
Assert-Missing $startMenu

$entryAfter = @(
    foreach ($key in $uninstallKeys) {
        if (Test-Path $key) {
            Get-ChildItem $key -ErrorAction SilentlyContinue |
                ForEach-Object { Get-ItemProperty $_.PSPath } |
                Where-Object { $_.DisplayName -eq $product }
        }
    }
)
if ($entryAfter) {
    throw "the uninstall entry survived"
}

Write-Host "PASS: silent install and uninstall"
