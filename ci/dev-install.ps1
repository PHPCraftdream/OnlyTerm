<#
.SYNOPSIS
  Build OnlyTerm in release mode and install/update it in place on this
  Windows machine, without going through the full Inno Setup packaging
  pipeline (ci/windows-installer.iss) -- for local dev iteration only.

.DESCRIPTION
  Builds onlyterm / onlyterm-gui / onlyterm-mux-server in --release mode,
  then hot-swaps the fresh binaries plus their runtime dependencies
  (conpty.dll, OpenConsole.exe, strip-ansi-escapes.exe) and .pdb files into
  the install directory, and makes sure Windows Error Reporting is
  configured to save a full crash dump for onlyterm-gui.exe if it ever
  crashes.

  "Hot-swap" means already-running OnlyTerm processes are never stopped:
  each destination file is renamed aside (to "<name>.old") instead of being
  overwritten in place, and the new file is written under the original
  name. Windows opens a running .exe/.dll with FILE_SHARE_DELETE by
  default, so renaming (or deleting) it while it's mapped into a live
  process is allowed -- the process keeps executing from the renamed file's
  still-open data, while any new process launched after this point picks up
  the fresh file at the original path. See Install-FileHotSwap below.

  Copying into "Program Files" requires administrator rights. This script
  re-launches itself elevated (one UAC prompt) if it isn't already running
  as admin, so you can always just run it directly, non-elevated, and
  approve the one prompt when it appears.

.PARAMETER InstallDir
  Where OnlyTerm is/should be installed. Defaults to the same default the
  Inno Setup installer uses: "$Env:ProgramFiles\OnlyTerm".

.PARAMETER SkipBuild
  Skip `cargo build --release` and just (re)install whatever is already in
  target\release. Useful if you just built it yourself a moment ago.

.EXAMPLE
  # from the repo root
  powershell -ExecutionPolicy Bypass -File ci\dev-install.ps1

.EXAMPLE
  # already built, just want to (re)install + fix the dump config
  powershell -ExecutionPolicy Bypass -File ci\dev-install.ps1 -SkipBuild
#>
[CmdletBinding()]
param(
    [string]$InstallDir = "$Env:ProgramFiles\OnlyTerm",
    [string]$ReleaseDir,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

# ---------------------------------------------------------------------------
# Re-launch elevated if we're not already admin. Everything after this point
# in the *elevated* copy of the script does the real work; the original,
# non-elevated invocation just waits for it and exits.
#
# `-Verb RunAs` opens a brand-new, detached console window for the elevated
# copy -- it does NOT share this process's stdout/stderr, so if something
# fails in there you'd otherwise never see why. Wrap the elevated run in
# Start-Transcript so a log always lands next to the script, and surface it
# here if the elevated run fails.
# ---------------------------------------------------------------------------
function Test-IsAdmin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltinRole]::Administrator)
}

$TranscriptPath = Join-Path $PSScriptRoot "dev-install.log"

if (-not (Test-IsAdmin)) {
    Write-Host "Not running elevated -- relaunching with a UAC prompt..." -ForegroundColor Yellow

    # Start-Process joins -ArgumentList with plain spaces and does NOT quote
    # anything for us, so any path containing a space (the default
    # "C:\Program Files\OnlyTerm" being the obvious one) has to carry its own
    # embedded quotes -- otherwise the elevated PowerShell sees
    # `-InstallDir C:\Program` plus a stray positional `Files\OnlyTerm`, fails
    # during *parameter binding* (i.e. before a single line of this script's
    # body runs, so before Start-Transcript can log anything) and exits 1 with
    # no diagnostics at all.
    $argList = @(
        "-NoProfile"
        "-ExecutionPolicy"; "Bypass"
        "-File"; "`"$PSCommandPath`""
        "-InstallDir"; "`"$InstallDir`""
    )
    if ($ReleaseDir) { $argList += @("-ReleaseDir"; "`"$ReleaseDir`"") }
    if ($SkipBuild) { $argList += "-SkipBuild" }

    # Stale log from a previous run would be misleading if this run dies
    # before it can write its own.
    Remove-Item -Force -ErrorAction SilentlyContinue $TranscriptPath

    $proc = Start-Process powershell -Verb RunAs -ArgumentList $argList -Wait -PassThru
    if ($proc.ExitCode -ne 0) {
        Write-Host "`nThe elevated run failed (exit code $($proc.ExitCode)). Log from that run:" -ForegroundColor Red
        if (Test-Path $TranscriptPath) {
            Get-Content $TranscriptPath | Write-Host
        } else {
            Write-Host "(no log at $TranscriptPath -- the elevated process died before it could start logging;"
            Write-Host " that usually means PowerShell rejected the command line itself, eg. a quoting problem)"
        }
    }
    exit $proc.ExitCode
}

Start-Transcript -Path $TranscriptPath -Force | Out-Null

try {

# ---------------------------------------------------------------------------
# Locate the repo root (this script lives in <repo>\ci\dev-install.ps1) and
# the build output directory. Respects a CARGO_TARGET_DIR override, since
# this machine's cargo config points the whole workspace at a shared
# out-of-tree target dir.
# ---------------------------------------------------------------------------
$RepoRoot = Split-Path -Parent $PSScriptRoot

if (-not $ReleaseDir) {
    $TargetDir = if ($Env:CARGO_TARGET_DIR) { $Env:CARGO_TARGET_DIR } else { Join-Path $RepoRoot "target" }
    $ReleaseDir = Join-Path $TargetDir "release"
}

Write-Host "Repo root:    $RepoRoot"
Write-Host "Release dir:  $ReleaseDir"
Write-Host "Install dir:  $InstallDir"

# ---------------------------------------------------------------------------
# 1. Build (unless -SkipBuild)
# ---------------------------------------------------------------------------
if (-not $SkipBuild) {
    Write-Host "`n==> cargo build --release -p wezterm -p wezterm-gui -p wezterm-mux-server" -ForegroundColor Cyan
    Push-Location $RepoRoot
    try {
        & cargo build --release -p wezterm -p wezterm-gui -p wezterm-mux-server
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed with exit code $LASTEXITCODE"
        }
    } finally {
        Pop-Location
    }
} else {
    Write-Host "`n==> Skipping build (-SkipBuild); using whatever is already in $ReleaseDir" -ForegroundColor Yellow
}

# ---------------------------------------------------------------------------
# 2. Hot-swap one file into place: rename any existing destination aside
#    (to "<name>.old") rather than overwriting it, so a process that
#    currently has it open/mapped keeps running against the renamed file's
#    data untouched, then write the new file under the original name.
#
#    Left-over "<name>.old" files from a *previous* install are cleaned up
#    opportunistically here too, on a best-effort basis: they can only be
#    deleted once every process still holding the old file open has exited,
#    which may not be true yet (e.g. a long-lived onlyterm-mux-server), so a
#    delete failure here is expected and not an error -- it'll be retried on
#    the next install.
# ---------------------------------------------------------------------------
function Install-FileHotSwap {
    param(
        [Parameter(Mandatory)][string]$Source,
        [Parameter(Mandatory)][string]$Destination
    )

    $oldPath = "$Destination.old"

    # Best-effort cleanup of a leftover ".old" from a prior install whose
    # owning process has since exited. Not fatal if it's still locked.
    if (Test-Path $oldPath) {
        Remove-Item -Force -ErrorAction SilentlyContinue $oldPath
    }

    if (Test-Path $Destination) {
        # If a ".old" from *this* run's own cleanup attempt above is still
        # around (still locked by a running process), Rename-Item would
        # collide with it -- fall back to a unique name in that rare case so
        # the hot-swap still succeeds rather than aborting the whole install.
        $renameTarget = $oldPath
        if (Test-Path $renameTarget) {
            $renameTarget = "$Destination.old.$([guid]::NewGuid().ToString('N').Substring(0, 8))"
        }
        Rename-Item -Force -Path $Destination -NewName (Split-Path -Leaf $renameTarget)
    }

    Copy-Item -Force $Source $Destination
}

# ---------------------------------------------------------------------------
# 3. Copy binaries + pdbs + runtime deps into the install dir
# ---------------------------------------------------------------------------
# The app genuinely can't run without these; a missing one means we're
# pointed at the wrong directory (or at a tree that was never built in
# release mode), which must be a hard error rather than a warning -- silently
# copying nothing and still reporting success is the worst possible outcome
# here, especially since a stale <repo>\target usually exists alongside a
# real out-of-tree CARGO_TARGET_DIR.
$requiredFiles = @(
    "onlyterm.exe",
    "onlyterm-gui.exe",
    "onlyterm-mux-server.exe",
    "conpty.dll", "OpenConsole.exe", "strip-ansi-escapes.exe"
)
# Debug symbols: nice for reading crash dumps, but not fatal if absent.
$optionalFiles = @(
    "onlyterm.pdb", "onlyterm_gui.pdb", "onlyterm_mux_server.pdb"
)

$missingRequired = $requiredFiles | Where-Object { -not (Test-Path (Join-Path $ReleaseDir $_)) }
if ($missingRequired) {
    throw @"
Release build not found in: $ReleaseDir
Missing required file(s): $($missingRequired -join ', ')

Check that the release build actually completed, and that the release
directory above is the right one. This machine builds out of tree via
CARGO_TARGET_DIR (currently: $(if ($Env:CARGO_TARGET_DIR) { $Env:CARGO_TARGET_DIR } else { '<unset>' })),
so a stale <repo>\target can look plausible while containing nothing useful.
You can point this script explicitly with:  -ReleaseDir <path>
"@
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

Write-Host "`n==> Hot-swapping binaries into $InstallDir (running OnlyTerm processes are left running)" -ForegroundColor Cyan
$copied = 0
foreach ($f in $requiredFiles) {
    Install-FileHotSwap -Source (Join-Path $ReleaseDir $f) -Destination (Join-Path $InstallDir $f)
    Write-Host "    $f"
    $copied++
}
foreach ($f in $optionalFiles) {
    $src = Join-Path $ReleaseDir $f
    if (Test-Path $src) {
        Install-FileHotSwap -Source $src -Destination (Join-Path $InstallDir $f)
        Write-Host "    $f"
        $copied++
    } else {
        Write-Warning "    $f not found -- skipped (debug symbols only)"
    }
}
Write-Host "    ($copied file(s) copied)"

# ---------------------------------------------------------------------------
# 4. Make sure WER LocalDumps is configured for onlyterm-gui.exe, so a crash
#    always leaves a full dump behind for post-mortem analysis.
# ---------------------------------------------------------------------------
Write-Host "`n==> Ensuring crash-dump collection is configured for onlyterm-gui.exe" -ForegroundColor Cyan
$dumpFolder = "C:\CrashDumps\OnlyTerm"
$werKey = "HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps\onlyterm-gui.exe"

New-Item -ItemType Directory -Force -Path $dumpFolder | Out-Null
if (-not (Test-Path $werKey)) {
    New-Item -Path $werKey -Force | Out-Null
}
New-ItemProperty -Path $werKey -Name "DumpFolder" -PropertyType ExpandString -Value $dumpFolder -Force | Out-Null
New-ItemProperty -Path $werKey -Name "DumpType" -PropertyType DWord -Value 2 -Force | Out-Null       # 2 = full dump
New-ItemProperty -Path $werKey -Name "DumpCount" -PropertyType DWord -Value 10 -Force | Out-Null      # keep the last 10

Write-Host "    DumpFolder = $dumpFolder"
Write-Host "    DumpType   = 2 (full dump)"
Write-Host "    DumpCount  = 10"

$installedGui = Join-Path $InstallDir "onlyterm-gui.exe"
$stamp = (Get-Item $installedGui).LastWriteTime
Write-Host "`nDone. Installed at $InstallDir (onlyterm-gui.exe built $stamp)." -ForegroundColor Green

} catch {
    Write-Host "`nFAILED: $($_.Exception.Message)" -ForegroundColor Red
    Write-Host $_.ScriptStackTrace
    Stop-Transcript | Out-Null
    exit 1
}

Stop-Transcript | Out-Null
