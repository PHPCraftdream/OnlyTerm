[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $GuiExe,
    [Parameter(Mandatory = $true)]
    [string] $CliExe,
    [ValidateSet('ascii', 'cyrillic', 'sgr', 'tui', 'history', 'bulk-printstring', 'bulk-lines', 'resize', 'multi-pane', 'input-probe')]
    [string] $Scenario = 'multi-pane',
    [string] $OutputDir = $null,
    [switch] $DisableInstrumentation,
    [switch] $ConcurrentBulk
)

$ErrorActionPreference = 'Stop'
if ($ConcurrentBulk -and $Scenario -ne 'input-probe') { throw 'ConcurrentBulk requires Scenario input-probe' }
if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $OutputDir = Join-Path $PSScriptRoot ("results\{0}\{1:yyyyMMdd-HHmmss-fff}" -f $Scenario, (Get-Date))
}
$GuiExe = (Resolve-Path $GuiExe).Path
$CliExe = (Resolve-Path $CliExe).Path
$OutputDir = [IO.Path]::GetFullPath($OutputDir)
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
$runRoot = Join-Path $OutputDir 'isolated-profile-home'
$localAppData = Join-Path $runRoot 'LocalAppData'
$roamingAppData = Join-Path $runRoot 'RoamingAppData'
New-Item -ItemType Directory -Force -Path $runRoot, $localAppData, $roamingAppData, (Join-Path $runRoot '.local\share\onlyterm') | Out-Null
$workload = Join-Path $PSScriptRoot 'workload.ps1'
if (-not (Test-Path $workload)) { throw "Missing workload fixture: $workload" }

$script:GuiPid = $null
$script:GuiProcess = $null
$script:CliPids = New-Object 'System.Collections.Generic.List[int]'
$script:TimelinePath = Join-Path $OutputDir 'timeline.csv'
$script:Timeline = New-Object 'System.Collections.Generic.List[object]'
$script:OwnedPaneIds = New-Object 'System.Collections.Generic.List[string]'
$script:ProcessTree = New-Object 'System.Collections.Generic.List[object]'
$script:ProbeSamples = New-Object 'System.Collections.Generic.List[object]'
$script:WorkerPids = New-Object 'System.Collections.Generic.List[int]'
$script:RuntimeDirectory = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.local\share\onlyterm'
$oldEnvironment = @{}
$isolatedEnvironment = @{
    USERPROFILE = $runRoot
    APPDATA = $roamingAppData
    LOCALAPPDATA = $localAppData
    HOME = $runRoot
    ONLYTERM_LOG = 'info'
    ONLYTERM_PROFILE_INTERVAL_SECONDS = $(if ($DisableInstrumentation) { '0' } else { '1' })
    ONLYTERM_UNIX_SOCKET = $null
    ONLYTERM_PANE = $null
    ONLYTERM_PROFILE_ACK_PATH = (Join-Path $OutputDir 'input-receipts.csv')
}
foreach ($name in $isolatedEnvironment.Keys) {
    $oldEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
    [Environment]::SetEnvironmentVariable($name, $isolatedEnvironment[$name], 'Process')
}

if (-not ('OnlyTermProfileWin32' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class OnlyTermProfileWin32 {
    [DllImport("user32.dll", SetLastError=true)]
    public static extern bool SetWindowPos(IntPtr hWnd, IntPtr hWndInsertAfter,
        int X, int Y, int cx, int cy, uint uFlags);
    [DllImport("user32.dll", SetLastError=true)]
    public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
    [DllImport("user32.dll")]
    public static extern uint MapVirtualKey(uint code, uint mapType);
}
'@
}

function ConvertTo-Win32Argument([string] $Value) {
    if ($Value -notmatch '[\s"]') { return $Value }
    return '"' + $Value.Replace('"', '\"') + '"'
}

function Add-Timeline([string] $Phase, [string] $Event, [string] $Detail) {
    $script:Timeline.Add([pscustomobject]@{
        timestamp_utc = [DateTimeOffset]::UtcNow.ToString('o')
        phase = $Phase
        event = $Event
        detail = $Detail
    })
}

function Invoke-OnlyTermCli([string[]] $CliArguments) {
    $allArguments = @('--skip-config', 'cli', '--no-auto-start', '--class', $script:WindowClass) + $CliArguments
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $CliExe
    $psi.Arguments = (($allArguments | ForEach-Object { ConvertTo-Win32Argument ([string]$_) }) -join ' ')
    $psi.WorkingDirectory = $OutputDir
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $psi.RedirectStandardInput = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $process = [System.Diagnostics.Process]::Start($psi)
    $cliPid = $process.Id
    [void]$script:CliPids.Add($cliPid)
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit(120000)) {
        # Exact onlyterm.exe PID captured from our Process.Start above.
        $process.Kill()
        $process.WaitForExit()
        throw "Harness-owned onlyterm CLI PID $cliPid timed out: $($psi.Arguments)"
    }
    $stdout = $stdoutTask.GetAwaiter().GetResult()
    $stderr = $stderrTask.GetAwaiter().GetResult()
    if ($process.ExitCode -ne 0) {
        throw "onlyterm CLI failed ($($process.ExitCode)): $($psi.Arguments)`n$stderr"
    }
    return $stdout
}

function Send-PaneCommand([string] $PaneId, [string] $Command) {
    $enter = ([string][char]27) + '[13;28;13;1;0;1_' + ([string][char]27) + '[13;28;13;0;0;1_'
    [void](Invoke-OnlyTermCli -CliArguments @('send-text', '--pane-id', $PaneId, '--no-paste', ($Command + $enter)))
}

function Read-ProbeReceipts {
    $stream = [IO.File]::Open($isolatedEnvironment.ONLYTERM_PROFILE_ACK_PATH, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
    $reader = New-Object IO.StreamReader($stream)
    try { $receiptText = $reader.ReadToEnd() }
    finally { $reader.Dispose() }
    $receiptText | ConvertFrom-Csv
}

function Wait-PaneText([string] $PaneId, [string] $Needle, [int] $TimeoutSeconds = 30) {
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        try {
            $text = Invoke-OnlyTermCli @('get-text', '--pane-id', $PaneId, '--start-line', '-80')
            if ($text.Contains($Needle)) { return }
        }
        catch { $lastCliError = $_.Exception.Message }
        Start-Sleep -Milliseconds 200
    }
    if ($null -ne $text) {
        Set-Content -LiteralPath (Join-Path $OutputDir ("pane-{0}-timeout.txt" -f $PaneId)) -Value $text -Encoding UTF8
    }
    throw "Timed out waiting for pane $PaneId to display '$Needle'; last CLI error: $lastCliError"
}

function Run-Phase([string] $Name, [scriptblock] $Start, [string[]] $PaneIds, [string[]] $Markers) {
    Add-Timeline $Name 'start' ''
    & $Start
    for ($i = 0; $i -lt $PaneIds.Count; $i++) {
        Wait-PaneText $PaneIds[$i] "PROFILE_PHASE_DONE $($Markers[$i])" 30
    }
    Add-Timeline $Name 'end' ''
}

function Get-ProcessDescendants([int] $RootPid) {
    $all = @(Get-CimInstance Win32_Process)
    $known = New-Object 'System.Collections.Generic.HashSet[int]'
    [void]$known.Add($RootPid)
    $found = New-Object 'System.Collections.Generic.List[object]'
    $changed = $true
    while ($changed) {
        $changed = $false
        foreach ($item in $all) {
            $parent = [int]$item.ParentProcessId
            $id = [int]$item.ProcessId
            if ($known.Contains($parent) -and -not $known.Contains($id)) {
                [void]$known.Add($id)
                $found.Add([pscustomobject]@{ pid = $id; parent_pid = $parent; image = $item.Name })
                $changed = $true
            }
        }
    }
    return $found
}

function Export-ProfileMetrics {
    $rows = New-Object 'System.Collections.Generic.List[object]'
    $ownedLogDir = Join-Path $OutputDir 'owned-logs'
    New-Item -ItemType Directory -Force -Path $ownedLogDir | Out-Null
    $guiLog = Join-Path $script:RuntimeDirectory ("onlyterm-gui.exe-log-{0}.txt" -f $script:GuiPid)
    if (Test-Path -LiteralPath $guiLog) {
        foreach ($line in Get-Content -LiteralPath $guiLog) {
            if ($line -match 'HostProcessBackend: generation \d+ running as PID (\d+)') {
                [void]$script:WorkerPids.Add([int]$Matches[1])
            }
        }
    }
    $ownedLogPids = @($script:GuiPid) + @($script:WorkerPids)
    foreach ($ownedPid in @($ownedLogPids | Sort-Object -Unique)) {
        $sourceLog = Join-Path $script:RuntimeDirectory ("onlyterm-gui.exe-log-{0}.txt" -f $ownedPid)
        if (Test-Path -LiteralPath $sourceLog) {
            Copy-Item -LiteralPath $sourceLog -Destination $ownedLogDir -Force
        }
    }
    $logFiles = @(Get-ChildItem -Path $ownedLogDir -Filter 'onlyterm-gui.exe-log-*.txt' -File)
    foreach ($file in $logFiles) {
        $logPid = 0
        if ($file.BaseName -match 'log-(\d+)$') { $logPid = [int]$Matches[1] }
        foreach ($line in Get-Content -LiteralPath $file.FullName) {
            if ($line -match 'HostProcessBackend: generation \d+ running as PID (\d+)') {
                [void]$script:WorkerPids.Add([int]$Matches[1])
            }
            if ($line -match 'PROFILE_METRIC metric=([^ ]+) unit=([^ ]+) samples=(\d+) mean=([^ ]+) p50=(\d+) p95=(\d+) p99=(\d+) max=(\d+)') {
                $rows.Add([pscustomobject]@{
                    pid = $logPid
                    metric = $Matches[1]
                    unit = $Matches[2]
                    samples = [long]$Matches[3]
                    mean = [double]::Parse($Matches[4], [System.Globalization.CultureInfo]::InvariantCulture)
                    estimated_total = [double]$Matches[3] * [double]::Parse($Matches[4], [System.Globalization.CultureInfo]::InvariantCulture)
                    p50 = [long]$Matches[5]
                    p95 = [long]$Matches[6]
                    p99 = [long]$Matches[7]
                    max = [long]$Matches[8]
                    log_file = $file.FullName
                })
            }
        }
    }
    $rows | Export-Csv -NoTypeInformation -Encoding UTF8 -Path (Join-Path $OutputDir 'metrics-intervals.csv')
    $latest = @{}
    foreach ($row in $rows) { $latest["$($row.pid)|$($row.metric)"] = $row }
    $latest.Values | Sort-Object pid, metric | Export-Csv -NoTypeInformation -Encoding UTF8 -Path (Join-Path $OutputDir 'metrics-latest.csv')
    if ($script:ProbeSamples.Count -gt 0) {
        $script:ProbeSamples | Export-Csv -NoTypeInformation -Encoding UTF8 -Path (Join-Path $OutputDir 'input-probes.csv')
    }

    $processRows = New-Object 'System.Collections.Generic.List[object]'
    if ($script:GuiPid) {
        foreach ($child in $script:ProcessTree) {
            $processRows.Add([pscustomobject]@{ pid = $child.pid; parent_pid = $child.parent_pid; image = $child.image; launched_by_harness = $false })
        }
    }
    if ($script:GuiPid) {
        $processRows.Add([pscustomobject]@{ pid = $script:GuiPid; parent_pid = ''; image = 'onlyterm-gui.exe'; launched_by_harness = $true })
    }
    foreach ($workerPid in $script:WorkerPids) {
        $processRows.Add([pscustomobject]@{ pid = $workerPid; parent_pid = $script:GuiPid; image = 'onlyterm-gui.exe'; launched_by_harness = $false })
    }
    foreach ($cliPid in $script:CliPids) {
        $processRows.Add([pscustomobject]@{ pid = $cliPid; parent_pid = $PID; image = 'onlyterm.exe'; launched_by_harness = $true })
    }
    $processRows | Sort-Object pid -Unique | Export-Csv -NoTypeInformation -Encoding UTF8 -Path (Join-Path $OutputDir 'processes.csv')
    $recordedPids = @($processRows | ForEach-Object { [int]$_.pid })
    if ($script:GuiPid -and ($recordedPids -notcontains [int]$script:GuiPid)) {
        throw "processes.csv is missing owned GUI PID $script:GuiPid"
    }
    foreach ($cliPid in $script:CliPids) {
        if ($recordedPids -notcontains [int]$cliPid) {
            throw "processes.csv is missing owned CLI PID $cliPid"
        }
    }
    foreach ($workerPid in $script:WorkerPids) {
        if ($recordedPids -notcontains [int]$workerPid) {
            throw "processes.csv is missing HostProcess worker PID $workerPid"
        }
    }
    $latestRows = @($latest.Values)
    $sampledMetrics = @($latestRows | Where-Object { $_.samples -gt 0 } | Select-Object -ExpandProperty metric -Unique)
    $requiredMetrics = @(
        'mux.pty.parse_and_buffer',
        'mux.pty.parse_bytes.size',
        'localpane.perform_actions.chunk_actions.size',
        'localpane.terminal_lock.wait.perform_actions',
        'localpane.terminal_lock.hold.perform_actions',
        'localpane.terminal_lock.wait.render_snapshot',
        'localpane.terminal_lock.hold.render_snapshot',
        'cached_cluster_shape',
        'gui.paint.collect',
        'gui.paint.collect.quads.size',
        'gui.paint.impl',
        'gui.host_process.wire_frame_build',
        'gui.host_process.ipc_enqueue',
        'gui.host_process.inflight_duration',
        'gui.host_process.frame_encode_write',
        'gui.host_process.frame_pipe_flush',
        'gui.host_process.worker_frame_build',
        'gui.host_process.worker_submit',
        'gui.host_process.worker_ack_write_flush'
    )
    if ($Scenario -eq 'resize') { $requiredMetrics += 'conpty.resize_pseudoconsole' }
    if ($Scenario -eq 'input-probe') {
        $requiredMetrics += 'gui.input_to_next_paint'
    }
    $missingMetrics = @($requiredMetrics | Where-Object { $sampledMetrics -notcontains $_ })
    if (-not $DisableInstrumentation -and $missingMetrics.Count -gt 0) {
        throw "Scenario '$Scenario' has no recorded samples for required stages: $($missingMetrics -join ', ')"
    }
    if ($Scenario -eq 'input-probe') {
        $unobserved = @($script:ProbeSamples | Where-Object { -not $_.application_received })
        if ($script:ProbeSamples.Count -ne 100 -or $unobserved.Count -gt 0) {
            throw "Input probe expected 100 application-received markers; got $($script:ProbeSamples.Count), unobserved=$($unobserved.Count)"
        }
        if ($ConcurrentBulk -and @($script:ProbeSamples | Where-Object { $_.bulk_output_active }).Count -ne 100) {
            throw 'Concurrent input probe requires all 100 receipts while the bulk writer is active'
        }
        $probeSummaryPath = Join-Path $OutputDir 'input-probe-summary.csv'
        if (-not (Test-Path $probeSummaryPath)) { throw 'Input probe summary is missing' }
        $probeSummary = Import-Csv -LiteralPath $probeSummaryPath | Select-Object -First 1
        if (-not $probeSummary -or [int]$probeSummary.samples -ne 100) {
            throw 'Input probe summary does not contain 100 samples'
        }
    }
}

$script:RunFailure = $null
$script:CleanupFailure = $null
$script:ArtifactFailure = $null
try {
    Add-Timeline 'run' 'start' "scenario=$Scenario; gui=$GuiExe; cli=$CliExe; output=$OutputDir"
    $script:WindowClass = "OnlyTermProfile-$Scenario-$PID-$([DateTime]::UtcNow.ToString('HHmmss'))"
    $paneCount = if (@('multi-pane', 'resize') -contains $Scenario) { 4 } else { 1 }
    $powershellExe = Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $guiArguments = @(
        '--skip-config', '--config', 'periodic_stat_logging=0',
        'start', '--always-new-process', '--class', $script:WindowClass, '--',
        $powershellExe, '-NoLogo', '-NoProfile', '-File', $workload
    )
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $GuiExe
    $psi.Arguments = (($guiArguments | ForEach-Object { ConvertTo-Win32Argument ([string]$_) }) -join ' ')
    $psi.WorkingDirectory = $OutputDir
    $psi.UseShellExecute = $false
    $script:GuiProcess = [System.Diagnostics.Process]::Start($psi)
    $script:GuiPid = $script:GuiProcess.Id
    Add-Timeline 'startup' 'gui_pid' ([string]$script:GuiPid)
    $script:OwnedSocket = Join-Path $script:RuntimeDirectory ("gui-sock-{0}" -f $script:GuiPid)
    [Environment]::SetEnvironmentVariable('ONLYTERM_UNIX_SOCKET', $script:OwnedSocket, 'Process')
    Add-Timeline 'startup' 'owned_socket' $script:OwnedSocket

    $windowDeadline = [DateTime]::UtcNow.AddSeconds(60)
    do {
        $script:GuiProcess.Refresh()
        if ($script:GuiProcess.HasExited) { throw "OnlyTerm GUI exited during startup: $($script:GuiProcess.ExitCode)" }
        $windowHandle = $script:GuiProcess.MainWindowHandle
        if ($windowHandle -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 200 }
    } while ($windowHandle -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $windowDeadline)
    if ($windowHandle -eq [IntPtr]::Zero) { throw 'Timed out waiting for the owned GUI process window handle' }

    $paneList = $null
    $listDeadline = [DateTime]::UtcNow.AddSeconds(120)
    while (-not $paneList -and [DateTime]::UtcNow -lt $listDeadline) {
        try {
            $candidate = Invoke-OnlyTermCli @('list', '--format', 'json') | ConvertFrom-Json
            $candidateRows = @($candidate)
            if ($candidateRows.Count -gt 0) {
                $firstPaneProperty = $candidateRows[0].PSObject.Properties['pane_id']
                if ($null -ne $firstPaneProperty -and $null -ne $firstPaneProperty.Value) {
                    $paneList = $candidateRows
                    $initialPaneId = [string]$firstPaneProperty.Value
                }
            }
        }
        catch { }
        if (-not $paneList) { Start-Sleep -Milliseconds 200 }
    }
    if (-not $paneList) { throw 'Timed out waiting for the initial pane list from the owned GUI' }
    $paneIds = New-Object 'System.Collections.Generic.List[string]'
    $paneIds.Add($initialPaneId)
    $script:OwnedPaneIds.Add($initialPaneId)
    Wait-PaneText $paneIds[0] 'PROFILE_READY' 90

    for ($index = 1; $index -lt $paneCount; $index++) {
        $direction = if ($index % 2) { '--right' } else { '--bottom' }
        $splitOut = Invoke-OnlyTermCli @(
            'split-pane', '--pane-id', $paneIds[0], $direction, '--',
            $powershellExe, '-NoLogo', '-NoProfile', '-File', $workload
        )
        $newPaneId = $splitOut.Trim()
        if (-not $newPaneId) { throw "split-pane returned no pane id: $splitOut" }
        $paneIds.Add($newPaneId)
        $script:OwnedPaneIds.Add($newPaneId)
        Wait-PaneText $newPaneId 'PROFILE_READY' 90
    }
    Add-Timeline 'startup' 'panes_ready' ($paneIds -join ',')
    Start-Sleep -Seconds 3 # fixed warmup; excluded from workload phase durations

    switch ($Scenario) {
        'ascii' {
            Run-Phase 'ascii_output' { Send-PaneCommand $paneIds[0] 'ascii' } @($paneIds[0]) @('ascii')
        }
        'cyrillic' {
            Run-Phase 'cyrillic_output' { Send-PaneCommand $paneIds[0] 'cyrillic' } @($paneIds[0]) @('cyrillic')
        }
        'sgr' {
            Run-Phase 'sgr_output' { Send-PaneCommand $paneIds[0] 'sgr' } @($paneIds[0]) @('sgr')
        }
        'tui' {
            Run-Phase 'tui_redraw' { Send-PaneCommand $paneIds[0] 'tui' } @($paneIds[0]) @('tui')
        }
        'history' {
            Run-Phase 'long_history' { Send-PaneCommand $paneIds[0] 'history' } @($paneIds[0]) @('history')
        }
        'bulk-printstring' {
            foreach ($kib in @(16, 64, 128, 512, 8192)) {
                $name = "bulk${kib}KiB"
                Run-Phase $name { Send-PaneCommand $paneIds[0] "bulk$kib" } @($paneIds[0]) @($name)
            }
        }
        'bulk-lines' {
            Run-Phase 'bulk_lines_sgr' { Send-PaneCommand $paneIds[0] 'bulk-lines' } @($paneIds[0]) @('bulk-lines')
        }
        'multi-pane' {
            Run-Phase 'parallel_text_output' {
                Send-PaneCommand $paneIds[0] 'ascii'
                Send-PaneCommand $paneIds[1] 'cyrillic'
                Send-PaneCommand $paneIds[2] 'sgr'
                Send-PaneCommand $paneIds[3] 'tui'
            } @($paneIds.ToArray()) @('ascii', 'cyrillic', 'sgr', 'tui')
            Run-Phase 'multi_pane_history' { Send-PaneCommand $paneIds[0] 'history' } @($paneIds[0]) @('history')
        }
        'resize' {
            Run-Phase 'resize_warmup' {
                Send-PaneCommand $paneIds[0] 'ascii'
                Send-PaneCommand $paneIds[1] 'cyrillic'
                Send-PaneCommand $paneIds[2] 'sgr'
                Send-PaneCommand $paneIds[3] 'tui'
            } @($paneIds.ToArray()) @('ascii', 'cyrillic', 'sgr', 'tui')
            Run-Phase 'resize_history' { Send-PaneCommand $paneIds[0] 'history' } @($paneIds[0]) @('history')

            Add-Timeline 'window_resize' 'start' ''
            foreach ($size in @(@(1320, 820), @(960, 820), @(960, 640), @(1480, 640), @(1480, 920), @(1120, 760))) {
                $resizeStart = [Diagnostics.Stopwatch]::StartNew()
                $ok = [OnlyTermProfileWin32]::SetWindowPos($windowHandle, [IntPtr]::Zero, 0, 0, $size[0], $size[1], 0x0014)
                if (-not $ok) { throw "SetWindowPos failed for $($size[0])x$($size[1]) (Win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error()))" }
                Start-Sleep -Milliseconds 500
                Add-Timeline 'window_resize' 'set_window_size' ("{0}x{1}; elapsed_ms={2:N3}" -f $size[0], $size[1], $resizeStart.Elapsed.TotalMilliseconds)
            }
            Add-Timeline 'window_resize' 'end' ''
        }
        'input-probe' {
            [void](Invoke-OnlyTermCli @('activate-pane', '--pane-id', $paneIds[0]))
            $script:GuiProcess.Refresh()
            $targetWindow = $script:GuiProcess.MainWindowHandle
            $probeCommand = "probe-keys $script:GuiPid" + $(if ($ConcurrentBulk) { ' bulk' } else { '' })
            Send-PaneCommand $paneIds[0] $probeCommand
            $readyDeadline = [DateTime]::UtcNow.AddSeconds(30)
            $ready = $null
            while ([DateTime]::UtcNow -lt $readyDeadline) {
                if (Test-Path -LiteralPath $isolatedEnvironment.ONLYTERM_PROFILE_ACK_PATH) {
                    $ready = Read-ProbeReceipts | Where-Object { $_.marker -eq "ready-$script:GuiPid" }
                    if ($null -ne $ready) { break }
                }
                Start-Sleep -Milliseconds 5
            }
            if ($null -eq $ready) { throw 'Timed out waiting for nonce-matched application readiness' }
            Add-Timeline 'input_probe' 'start' "100 owned-HWND F13 events; concurrent_bulk=$ConcurrentBulk; application receipt uses shared QPC"
            for ($i = 0; $i -lt 100; $i++) {
                $marker = "{0}{1:D3}" -f $script:GuiPid, $i
                $watch = [Diagnostics.Stopwatch]::StartNew()
                $enqueueQpc = [Diagnostics.Stopwatch]::GetTimestamp()
                $keys = @([uint32]124) # F13 avoids layout-dependent text translation.
                foreach ($virtualKey in $keys) {
                    [uint32]$windowOwner = 0
                    [void][OnlyTermProfileWin32]::GetWindowThreadProcessId($targetWindow, [ref]$windowOwner)
                    if ($windowOwner -ne $script:GuiPid) { throw 'Owned GUI HWND no longer belongs to the launched PID' }
                    $scan = [OnlyTermProfileWin32]::MapVirtualKey($virtualKey, 0)
                    $down = [int64](1 -bor ($scan -shl 16))
                    $up = [int64]$down -bor 0xC0000000L
                    if (-not [OnlyTermProfileWin32]::PostMessage($targetWindow, 0x100, [IntPtr]$virtualKey, [IntPtr]$down)) { throw 'Owned key-down enqueue failed' }
                    if (-not [OnlyTermProfileWin32]::PostMessage($targetWindow, 0x101, [IntPtr]$virtualKey, [IntPtr]$up)) { throw 'Owned key-up enqueue failed' }
                }
                $deadline = [DateTime]::UtcNow.AddSeconds(30)
                $receipt = $null
                while ([DateTime]::UtcNow -lt $deadline) {
                    $receipt = Read-ProbeReceipts | Where-Object {
                        $_.marker -eq $marker -and $_.qpc -match '^\d+$' -and $_.bulk_active -in @('True', 'False')
                    } | Select-Object -First 1
                    if ($null -ne $receipt) { break }
                    Start-Sleep -Milliseconds 5
                }
                if ($null -eq $receipt) { throw "Timed out waiting for application receipt $marker" }
                $receiptMs = ([long]$receipt.qpc - $enqueueQpc) * 1000.0 / [Diagnostics.Stopwatch]::Frequency
                if ($receiptMs -lt 0) { throw "Application receipt precedes enqueue for $marker" }
                $modelObserved = $false
                if (-not $ConcurrentBulk) {
                    Wait-PaneText $paneIds[0] "PROFILE_INPUT_ECHO $marker"
                    $modelObserved = $true
                }
                $watch.Stop()
                $sample = [pscustomobject]@{
                    sample = $i
                    marker = $marker
                    elapsed_ms = $watch.Elapsed.TotalMilliseconds
                    model_observed = $modelObserved
                    application_received = $true
                    application_receipt_ms = $receiptMs
                    bulk_output_active = $receipt.bulk_active -eq 'True'
                    endpoint = 'owned HWND enqueue to Console.ReadKey shared-QPC receipt; model observation recorded separately'
                    poll_period_ms = 5
                }
                $script:ProbeSamples.Add($sample)
                Add-Timeline 'input_probe' 'application_receipt_observed' ("marker=$marker; receipt_ms={0:N4}; model_observed=$modelObserved; observer_ms={1:N3}" -f $receiptMs, $watch.Elapsed.TotalMilliseconds)
                Start-Sleep -Milliseconds 100
            }
            Wait-PaneText $paneIds[0] 'PROFILE_PROBE_DONE count=100'
            $appLatencies = @($script:ProbeSamples | Sort-Object application_receipt_ms | ForEach-Object { [double]$_.application_receipt_ms })
            [pscustomobject]@{
                samples = $appLatencies.Count
                bulk_active_samples = @($script:ProbeSamples | Where-Object { $_.bulk_output_active }).Count
                p50_ms = $appLatencies[49]
                p95_ms = $appLatencies[94]
                p99_ms = $appLatencies[98]
                max_ms = $appLatencies[99]
                endpoint = 'Owned HWND F13 enqueue to ConPTY Console.ReadKey shared-QPC receipt; excludes observer polling and file I/O; not physical-keyboard or displayed-pixel latency'
            } | Export-Csv -NoTypeInformation -Encoding UTF8 -Path (Join-Path $OutputDir 'input-app-receipt-summary.csv')
            $sortedLatencies = @($script:ProbeSamples | Sort-Object elapsed_ms | ForEach-Object { [double]$_.elapsed_ms })
            $sampleCount = $sortedLatencies.Count
            $p50Index = [int][Math]::Max(0, [Math]::Ceiling(0.50 * $sampleCount) - 1)
            $p95Index = [int][Math]::Max(0, [Math]::Ceiling(0.95 * $sampleCount) - 1)
            $p99Index = [int][Math]::Max(0, [Math]::Ceiling(0.99 * $sampleCount) - 1)
            [pscustomobject]@{
                samples = $sampleCount
                p50_ms = $sortedLatencies[$p50Index]
                p95_ms = $sortedLatencies[$p95Index]
                p99_ms = $sortedLatencies[$p99Index]
                max_ms = $sortedLatencies[$sampleCount - 1]
                endpoint = $(if ($ConcurrentBulk) { 'Owned HWND enqueue to 5-ms application-receipt file observation; includes file I/O and observer polling; not model or pixel latency' } else { 'Owned HWND enqueue to CLI get-text model observation; includes receipt polling, CLI startup and 50-ms model polling; not displayed-pixel latency' })
            } | Export-Csv -NoTypeInformation -Encoding UTF8 -Path (Join-Path $OutputDir 'input-probe-summary.csv')
            Add-Timeline 'input_probe' 'end' ("samples=$sampleCount")
        }
    }

    for ($index = 0; $index -lt $paneIds.Count; $index++) {
        $paneText = Invoke-OnlyTermCli @('get-text', '--pane-id', $paneIds[$index], '--start-line', '-5000')
        $paneText | Set-Content -Encoding UTF8 -Path (Join-Path $OutputDir "pane-$index.txt")
    }

    Add-Timeline 'settle' 'start' 'Two profile intervals after model completion; not part of workload latency'
    Start-Sleep -Seconds 2
    Add-Timeline 'settle' 'end' ''

    Add-Timeline 'run' 'end' ''
    $script:ProcessTree = Get-ProcessDescendants $script:GuiPid
    Add-Timeline 'run' 'captured_descendant_pids' (($script:ProcessTree | ForEach-Object { $_.pid }) -join ',')
    $script:Timeline | Export-Csv -NoTypeInformation -Encoding UTF8 -Path $script:TimelinePath
}
catch {
    $script:RunFailure = $_
}
finally {
    # Close only the GUI instance this harness started. Its PID was captured
    # directly from Process.Start; no process discovery by image name is used.
    if ($script:GuiPid -and $script:GuiProcess) {
        try {
            if ($script:ProcessTree.Count -eq 0) {
                $script:ProcessTree = Get-ProcessDescendants $script:GuiPid
            }
        }
        catch { $script:CleanupFailure = $_ }
        try {
            $script:GuiProcess.Refresh()
            if (-not $script:GuiProcess.HasExited) {
                $hwnd = $script:GuiProcess.MainWindowHandle
                if ($hwnd -ne [IntPtr]::Zero) {
                    [void][OnlyTermProfileWin32]::PostMessage($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) # WM_CLOSE
                }
                if (-not $script:GuiProcess.WaitForExit(10000)) {
                    # Exact PID captured from our own Process.Start above; never a name/PID discovered from the system.
                    Stop-Process -Id $script:GuiPid -Force
                    $script:GuiProcess.WaitForExit()
                }
            }
        }
        catch { $script:CleanupFailure = $_ }
    }
    try {
        $script:Timeline | Export-Csv -NoTypeInformation -Encoding UTF8 -Path $script:TimelinePath
        Export-ProfileMetrics
    }
    catch { $script:ArtifactFailure = $_ }
    foreach ($name in $isolatedEnvironment.Keys) {
        try {
            [Environment]::SetEnvironmentVariable($name, $oldEnvironment[$name], 'Process')
        }
        catch { if (-not $script:CleanupFailure) { $script:CleanupFailure = $_ } }
    }
}

$failures = New-Object 'System.Collections.Generic.List[string]'
if ($script:RunFailure) { $failures.Add("Profile workload failed: $($script:RunFailure.Exception.Message)") }
if ($script:CleanupFailure) { $failures.Add("Owned-process cleanup failed: $($script:CleanupFailure.Exception.Message)") }
if ($script:ArtifactFailure) { $failures.Add("Profile artifact validation/export failed: $($script:ArtifactFailure.Exception.Message)") }
if ($failures.Count -gt 0) { throw ($failures -join [Environment]::NewLine) }

Write-Host "GUI profile scenario '$Scenario' results: $OutputDir"
Write-Host 'Artifacts: timeline.csv, metrics-intervals.csv, metrics-latest.csv, processes.csv; input-probes.csv and input-probe-summary.csv for input-probe; isolated-profile-home (per-PID logs)'
