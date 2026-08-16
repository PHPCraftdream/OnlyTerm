#!/bin/sh
# Neutral receiver for the "Ctrl+<letter> does nothing under a Cyrillic
# layout" investigation.
#
# OnlyTerm's own logs prove what it WRITES into the pty. They cannot show
# what the child actually RECEIVES, and the round trip in between is not a
# pipe: ConPTY parses our input into INPUT_RECORDs and conhost turns those
# back into bytes for the child. That conversion is the one stage that can
# consult the active keyboard layout, and it is invisible from both ends.
#
# This script is the missing end. It asks for win32-input-mode itself
# (DECSET 9001), exactly as Codex CLI does, so OnlyTerm encodes for it the
# same way -- then prints every byte that arrives, verbatim.
#
# Read the output like this: one press of Ctrl+J should show
#   ^[[74;36;10;1;8;1_    (key down)
#   ^[[74;36;10;0;8;1_    (key up)
# and Ctrl+C should show 67;46;3 in place of 74;36;10. Anything else --
# different numbers, Cyrillic bytes, nothing at all -- is the answer.
#
# stty raw also disables ISIG, so Ctrl+C is delivered as data instead of
# killing this script. Close the tab to finish.

printf 'keyprobe: press Ctrl+J and Ctrl+C, switch layouts, watch the bytes.\r\n'
printf 'keyprobe: expected for Ctrl+J: ^[[74;36;10;1;8;1_ then ^[[74;36;10;0;8;1_\r\n'
printf 'keyprobe: expected for Ctrl+C: ^[[67;46;3;1;8;1_  then ^[[67;46;3;0;8;1_\r\n'
printf 'keyprobe: close the tab when done.\r\n\r\n'

stty raw -echo
printf '\033[?9001h'
cat -v
