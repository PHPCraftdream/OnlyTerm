# Neutral receiver for the "Ctrl+<letter> does nothing under a Cyrillic
# layout" investigation -- the CONSOLE RECORD channel.
#
# The earlier keyprobe.sh read the byte stream (ReadFile on the console
# handle) and showed our win32-input-mode sequence arriving intact under
# both layouts. That measured the wrong channel: an application that reads
# the console with ReadConsoleInputW never sees those bytes. It sees
# INPUT_RECORDs that conhost reconstructs, and the UnicodeChar field of a
# reconstructed record is exactly the thing conhost can re-derive from the
# virtual key using the layout that happens to be active.
#
# $host.UI.RawUI.ReadKey surfaces that record verbatim -- VirtualKeyCode,
# Character and ControlKeyState are the KEY_EVENT_RECORD fields.
#
# Read it like this. For Ctrl+J the record should be
#   vk=74 (0x4A)  ch=10 (0x000A)  state contains LeftCtrlPressed
# and for Ctrl+C
#   vk=67 (0x43)  ch=3  (0x0003)
# If ch comes out as a Cyrillic codepoint (0x043E is 'о', 0x0441 is 'с')
# or as 0 under the Russian layout while being correct under the English
# one, that is the whole bug, and it happens after we hand the keypress
# over.
#
# Close the tab to finish: Ctrl+C is being captured as data, so it will
# not stop this loop.

# Without this, Ctrl+C is handled by the console as an interrupt and kills
# this process before the second half of the comparison can be typed --
# which is exactly what happened on the first run, leaving a Russian-layout
# capture with nothing to compare it against.
[Console]::TreatControlCAsInput = $true

Write-Host "recprobe: press Ctrl+J, Ctrl+Enter, Ctrl+C -- then switch layout and repeat."
Write-Host "recprobe: expected Ctrl+J/Ctrl+Enter -> vk=74 ch=10   Ctrl+C -> vk=67 ch=3"
Write-Host "recprobe: Ctrl+C will NOT stop this probe now. Close the tab when done."
Write-Host ""

# Wrapped so that a host that refuses ReadKey leaves its reason on screen
# instead of exiting the pane and taking the window with it -- the first
# run of this probe vanished without a trace, which is indistinguishable
# from "the user closed it".
try {
    while ($true) {
        $k = $host.UI.RawUI.ReadKey("NoEcho,IncludeKeyDown")
        $vk = [int]$k.VirtualKeyCode
        $ch = [int][char]$k.Character
        $line = "vk={0} (0x{1}) ch={2} (0x{3}) state={4}" -f `
            $vk, $vk.ToString("X2"), $ch, $ch.ToString("X4"), $k.ControlKeyState
        Write-Host $line
    }
}
catch {
    Write-Host ""
    Write-Host "recprobe: ReadKey failed -- $($_.Exception.Message)"
    Write-Host "recprobe: this host cannot read console records; tell the operator."
    Start-Sleep -Seconds 3600
}
