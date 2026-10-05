$ErrorActionPreference = 'Stop'
[Console]::InputEncoding = New-Object System.Text.UTF8Encoding($false)
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$esc = [char]27

function Write-PhaseDone([string] $Name) {
    [Console]::WriteLine("PROFILE_PHASE_DONE $Name")
    [Console]::Out.Flush()
}

function Run-Lines([string] $Name, [int] $Count, [string] $Text, [bool] $Sgr) {
    for ($i = 0; $i -lt $Count; $i++) {
        if ($Sgr) {
            [Console]::Write("$($esc)[38;5;{0}m" -f ($i % 256))
        }
        [Console]::WriteLine(("{0} {1:D6} {2}" -f $Name, $i, $Text))
        if ($Sgr) {
            [Console]::Write("$($esc)[0m")
        }
    }
    [Console]::Out.Flush()
    Write-PhaseDone $Name
}

function Run-TuiRedraw {
    $rows = 24
    for ($frame = 0; $frame -lt 240; $frame++) {
        [Console]::Write("$($esc)[2J$($esc)[H")
        [Console]::WriteLine("OnlyTerm profile TUI redraw frame {0:D4}" -f $frame)
        for ($row = 1; $row -lt $rows; $row++) {
            [Console]::WriteLine(("{0:D2} | status={1} | worker={2:D3} | redraw payload {3}" -f $row, $(if (($frame + $row) % 2) { 'busy' } else { 'idle' }), (($frame * 7 + $row) % 1000), ('界' * 16)))
        }
        [Console]::Out.Flush()
        Start-Sleep -Milliseconds 12
    }
    Write-PhaseDone 'tui'
}

function Run-History {
    for ($i = 0; $i -lt 8000; $i++) {
        [Console]::WriteLine(("history {0:D6} ASCII row; кириллица строка; wide=界; attrs={1}" -f $i, ($i % 16)))
    }
    [Console]::Out.Flush()
    Write-PhaseDone 'history'
}

function Run-Bulk([int] $Kib) {
    # One controlled printable run per call stresses a large PrintString; the
    # exact byte count is emitted alongside the content for run-to-run review.
    $bytes = $Kib * 1024
    $payload = 'x' * $bytes
    [Console]::Write($payload)
    [Console]::WriteLine(("`r`nPROFILE_BULK_DONE {0}KiB bytes={1}" -f $Kib, $bytes))
    [Console]::Out.Flush()
    Write-PhaseDone "bulk${Kib}KiB"
}

function Run-BulkLines {
    $body = 'R' * 480
    for ($i = 0; $i -lt 1024; $i++) {
        [Console]::Write("$($esc)[38;5;{0}m" -f ($i % 256))
        [Console]::WriteLine(("row {0:D4} {1}" -f $i, $body))
        [Console]::Write("$($esc)[0m")
    }
    [Console]::WriteLine("PROFILE_BULK_LINES rows=1024 printable_chars_per_row=$($body.Length)")
    [Console]::Out.Flush()
    Write-PhaseDone 'bulk-lines'
}

Add-Type -TypeDefinition @'
using System;
using System.Threading;
public sealed class OnlyTermProbeBulkWriter : IDisposable {
    private readonly ManualResetEventSlim stop = new ManualResetEventSlim(false);
    private readonly Thread thread;
    public bool Active { get { return thread.IsAlive; } }
    public OnlyTermProbeBulkWriter() {
        thread = new Thread(() => {
            string payload = "\x1b[31m" + new string('B', 64 * 1024) + "\x1b[0m";
            for (int i = 0; i < 1500 && !stop.Wait(20); i++) {
                Console.Write(payload);
                Console.Out.Flush();
            }
        });
        thread.IsBackground = true;
        thread.Start();
    }
    public void Dispose() {
        stop.Set();
        if (!thread.Join(5000)) throw new TimeoutException("Bulk writer did not stop");
        stop.Dispose();
    }
}
'@

[Console]::WriteLine('PROFILE_READY')
[Console]::Out.Flush()
while ($true) {
    $command = [Console]::ReadLine()
    if ($null -eq $command) { break }
    switch -Regex ($command.Trim()) {
        '^ascii$' { Run-Lines 'ascii' 1500 'The quick brown fox 0123456789' $false; break }
        '^cyrillic$' { Run-Lines 'cyrillic' 1500 'Съешь ещё этих мягких французских булок, да выпей чаю' $false; break }
        '^sgr$' { Run-Lines 'sgr' 1500 'SGR foreground/background bold underline reset' $true; break }
        '^tui$' { Run-TuiRedraw; break }
        '^history$' { Run-History; break }
        '^probe-keys ([0-9]+)( bulk)?$' {
            $nonce = $Matches[1]
            $bulk = if ($Matches[2]) { New-Object OnlyTermProbeBulkWriter } else { $null }
            [Console]::WriteLine('PROFILE_PROBE_READY')
            [Console]::Out.Flush()
            [IO.File]::WriteAllText($env:ONLYTERM_PROFILE_ACK_PATH, "marker,qpc,bulk_active`nready-$nonce,0,False`n")
            try {
                for ($i = 0; $i -lt 100;) {
                    $key = [Console]::ReadKey($true)
                    if ($key.Key -ne [ConsoleKey]::F13) { continue }
                    $received = [Diagnostics.Stopwatch]::GetTimestamp()
                    $marker = "{0}{1:D3}" -f $nonce, $i
                    $active = $null -ne $bulk -and $bulk.Active
                    [IO.File]::AppendAllText($env:ONLYTERM_PROFILE_ACK_PATH, "$marker,$received,$active`n")
                    if ($null -eq $bulk) {
                        [Console]::WriteLine("PROFILE_INPUT_ECHO $marker")
                        [Console]::Out.Flush()
                    }
                    $i++
                }
            }
            finally { if ($null -ne $bulk) { $bulk.Dispose() } }
            [Console]::WriteLine()
            [Console]::WriteLine('PROFILE_PROBE_DONE count=100')
            [Console]::Out.Flush()
            break
        }
        '^bulk-lines$' { Run-BulkLines; break }
        '^bulk(16|64|128|512|8192)$' { Run-Bulk ([int]$Matches[1]); break }
        '^quit$' { break }
        default { [Console]::WriteLine(("unknown profile workload: [{0}] codes=[{1}]" -f $command, (($command.ToCharArray() | ForEach-Object { [int]$_ }) -join ','))) }
    }
}
