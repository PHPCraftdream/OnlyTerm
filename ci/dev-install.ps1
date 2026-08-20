<#
.SYNOPSIS
  Build OnlyTerm in release mode and install/update it, including Explorer context
  menu entries, without the full Inno Setup packaging pipeline -- for local dev
  iteration only.

.DESCRIPTION
  Builds onlyterm / onlyterm-gui / onlyterm-mux-server in --release mode,
  then hot-swaps the fresh binaries plus their runtime dependencies
  (conpty.dll, OpenConsole.exe, strip-ansi-escapes.exe) and .pdb files into
  the install directory, configures Windows Error Reporting for full crash
  dumps, and registers Explorer context menu entries ("Open OnlyTerm here" and
  "OnlyTerm Run As") for Drive, Directory, and Directory\Background.

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
    # strip-ansi-escapes is its own workspace member, not a dependency of the
    # other three -- omitting it here used to work only by accident, because
    # a leftover release/ directory from an earlier full-workspace build
    # still had it. The first time this ran against a clean/relocated
    # CARGO_TARGET_DIR it wasn't there, and step 3 below failed on a missing
    # required file after a successful 14-minute build. Building it
    # explicitly removes that dependency on leftover state.
    Write-Host "`n==> cargo build --release -p wezterm -p wezterm-gui -p wezterm-mux-server -p strip-ansi-escapes" -ForegroundColor Cyan
    Push-Location $RepoRoot
    try {
        & cargo build --release -p wezterm -p wezterm-gui -p wezterm-mux-server -p strip-ansi-escapes
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
#    A binary and its .pdb are retired *together*, under one shared suffix.
#    That pairing is the whole point: a crash dump names the image it was
#    taken from, and the debugger then needs that build's symbols, not the
#    newest ones. Retiring them independently (which is what happens if each
#    file is hot-swapped on its own) quietly loses the symbols, because the
#    two files have opposite locking behaviour -- a running OnlyTerm keeps
#    its .exe mapped, so the old .exe survives every install, while nothing
#    holds a .pdb open, so the old .pdb is deleted and replaced each time.
#    The result is a directory full of retired binaries with symbols for
#    only the last one or two of them, discovered the hard way while trying
#    to read a dump from a process that had been running for three days.
function Install-FileHotSwap {
    param(
        [Parameter(Mandatory)][string]$Source,
        [Parameter(Mandatory)][string]$Destination,
        # Optional symbol file to retire under the same suffix as, and
        # install alongside, $Destination.
        [string]$SymbolSource,
        [string]$SymbolDestination
    )

    $suffix = $null
    if (Test-Path $Destination) {
        # Prefer the plain ".old" so the common case stays readable; fall
        # back to a unique suffix once ".old" is taken (which it will be as
        # soon as a running process has pinned one).
        $suffix = ".old"
        if (Test-Path "$Destination$suffix") {
            $suffix = ".old.$([guid]::NewGuid().ToString('N').Substring(0, 8))"
        }
        Rename-Item -Force -Path $Destination -NewName (Split-Path -Leaf "$Destination$suffix")
    }

    if ($SymbolDestination -and $suffix -and (Test-Path $SymbolDestination)) {
        $retiredSymbol = "$SymbolDestination$suffix"
        Remove-Item -Force -ErrorAction SilentlyContinue $retiredSymbol
        Rename-Item -Force -Path $SymbolDestination -NewName (Split-Path -Leaf $retiredSymbol)
    }

    Copy-Item -Force $Source $Destination
    if ($SymbolSource -and $SymbolDestination) {
        Copy-Item -Force $SymbolSource $SymbolDestination
    }
}

# Retired generations are ~50 MB of binary plus ~250 MB of symbols each, so
# they cannot simply accumulate. Prune oldest-first, but only ones that are
# genuinely unused: a retired .exe still mapped by a running OnlyTerm cannot
# be deleted, and that failure is exactly the signal to keep it (and its
# symbols) around. $Keep generations are kept beyond that, so a dump written
# shortly before an install can still be read afterwards.
function Remove-StaleRetiredGenerations {
    param(
        [Parameter(Mandatory)][string]$InstallDir,
        [Parameter(Mandatory)][string]$BinaryName,
        [string]$SymbolName,
        [int]$Keep = 5
    )

    $retired = @(Get-ChildItem -Path $InstallDir -Filter "$BinaryName.old*" -File -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending)
    if ($retired.Count -le $Keep) {
        return 0
    }

    $removed = 0
    foreach ($old in ($retired | Select-Object -Skip $Keep)) {
        $suffix = $old.Name.Substring($BinaryName.Length)
        Remove-Item -Force -ErrorAction SilentlyContinue $old.FullName
        if (Test-Path $old.FullName) {
            # Still mapped by a live process: keep its symbols too.
            continue
        }
        $removed++
        if ($SymbolName) {
            Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $InstallDir "$SymbolName$suffix")
        }
    }
    return $removed
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
# Debug symbols, keyed by the binary they belong to so that the two are
# retired together (see Install-FileHotSwap). Missing symbols are a warning,
# not an error -- the install is still usable, just not debuggable.
$symbolFor = @{
    "onlyterm.exe"            = "onlyterm.pdb"
    "onlyterm-gui.exe"        = "onlyterm_gui.pdb"
    "onlyterm-mux-server.exe" = "onlyterm_mux_server.pdb"
}

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
    $installArgs = @{
        Source      = (Join-Path $ReleaseDir $f)
        Destination = (Join-Path $InstallDir $f)
    }
    $pdb = $symbolFor[$f]
    if ($pdb) {
        if (Test-Path (Join-Path $ReleaseDir $pdb)) {
            $installArgs.SymbolSource = (Join-Path $ReleaseDir $pdb)
            $installArgs.SymbolDestination = (Join-Path $InstallDir $pdb)
        } else {
            Write-Warning "    $pdb not found -- installing $f without symbols; a crash dump from this build will not be readable"
        }
    }
    Install-FileHotSwap @installArgs
    if ($installArgs.SymbolSource) {
        Write-Host "    $f (+ $pdb)"
        $copied += 2
    } else {
        Write-Host "    $f"
        $copied++
    }
}
Write-Host "    ($copied file(s) copied)"

$pruned = 0
foreach ($f in $symbolFor.Keys) {
    $pruned += Remove-StaleRetiredGenerations -InstallDir $InstallDir -BinaryName $f -SymbolName $symbolFor[$f]
}
if ($pruned -gt 0) {
    Write-Host "    (pruned $pruned retired generation(s) no longer mapped by any running process)"
}

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

# ---------------------------------------------------------------------------
# 5. Install/update Explorer context menu entries (Open OnlyTerm here and
#    OnlyTerm Run As) for Drive, Directory, and Directory\Background.
#
#    HKA (in the .iss) maps to HKLM for an elevated install. We update rather
#    than fail if keys already exist, since a dev install after a real install
#    is the normal case, and the exe path may have changed.
#
#    The %V tail differs per scope, and all three are deliberate. They are
#    copied from ci/windows-installer.iss so that a dev install behaves like a
#    real one. Explorer substitutes a different shape of path per scope, and
#    CommandLineToArgvW then reads `\\` as one backslash and `\"` as a literal
#    quote, so each tail lands on the same result -- the clicked folder:
#      Drive       %V = C:\      -> "%V\"  is "C:\\"     -> C:\
#      Directory   %V = C:\foo   -> "%V\\" is "C:\foo\\" -> C:\foo\
#      Background  %V = C:\foo   -> "%V    is unterminated, which that parser
#                                   reads as "to end of line" -> C:\foo
#
#    The argument strings are single-quoted deliberately. They are full of
#    quotes and backslashes, and getting the escaping wrong in a double-quoted
#    string does NOT fail loudly: an unterminated string simply swallows the
#    following lines until it finds a closing quote, and the file still parses
#    clean. That exact mistake was made here once already.
# ---------------------------------------------------------------------------
Write-Host "`n==> Installing/updating Explorer context menu entries" -ForegroundColor Cyan
$exePath = Join-Path $InstallDir "onlyterm-gui.exe"
$entriesInstalled = 0

# Helper function: idempotently create/update a context menu entry
function Install-ContextMenuEntry {
    param(
        [Parameter(Mandatory)][string]$Scope,        # "Drive", "Directory", or "Directory\Background"
        [Parameter(Mandatory)][string]$Label,        # e.g., "Open OnlyTerm here" or "OnlyTerm Run As"
        [Parameter(Mandatory)][string]$ArgumentList  # arguments for the exe, including %V quoting
    )

    $keyPath = "HKLM:\SOFTWARE\Classes\$Scope\shell\$Label"
    $commandKeyPath = "$keyPath\command"

    # Create/update the main key and icon value
    if (-not (Test-Path $keyPath)) {
        New-Item -Path $keyPath -Force | Out-Null
    }
    Set-ItemProperty -Path $keyPath -Name "icon" -Value $exePath -Force | Out-Null

    # Create/update the command subkey
    $command = "`"$exePath`" $ArgumentList"
    if (-not (Test-Path $commandKeyPath)) {
        New-Item -Path $commandKeyPath -Force | Out-Null
    }
    Set-ItemProperty -Path $commandKeyPath -Name "(default)" -Value $command -Force | Out-Null
}

# One row per scope; the two labels differ only by `--choose-tab`, which makes
# OnlyTerm open the New Tab Options dialog instead of a tab, so the user picks
# shell / elevation / priority and gets exactly one tab of that kind -- in the
# folder they right-clicked.
$scopeTails = @(
    @{ Scope = 'Drive';                Tail = '--cwd "%V\"'   }
    @{ Scope = 'Directory';            Tail = '--cwd "%V\\"'  }
    @{ Scope = 'Directory\Background'; Tail = '--cwd "%V'     }
)

foreach ($s in $scopeTails) {
    foreach ($e in @(
        @{ Label = 'Open OnlyTerm here'; Extra = '' }
        @{ Label = 'OnlyTerm Run As';    Extra = '--choose-tab ' }
    )) {
        # Not `$args`: that is an automatic variable in PowerShell.
        $argLine = "start --no-auto-connect $($e.Extra)$($s.Tail)"
        Install-ContextMenuEntry -Scope $s.Scope -Label $e.Label -ArgumentList $argLine
        Write-Host "    $($s.Scope)\shell\$($e.Label)"
        $entriesInstalled++
    }
}

Write-Host "    ($entriesInstalled entry(s) installed/updated)"

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
