#!/bin/bash

# Update files that are derived from things baked into the executable

# Shell completion generation removed for Windows-only fork
# (bash/zsh/fish completions are not needed on Windows)

# Use the shared cargo target directory (CARGO_TARGET_DIR) if set,
# otherwise fall back to the default local target directory
TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"

for mode in copy_mode search_mode ; do
  fname="docs/examples/default-$(echo $mode | tr _ -)-key-table.markdown"
  # Wrap the KTAV output in a markdown code block
  echo "\`\`\`" > $fname
  $TARGET_DIR/debug/onlyterm -n show-keys --ktav --key-table $mode >> $fname
  echo "\`\`\`" >> $fname
done

# For whatever reason, running --help on macOS vs. Linux results in different
# opinions on leading/trailing whitespace. In order to minimize diffs and
# be more consistent, explicitly trim leading/trailing space from the
# output stream.
# <https://unix.stackexchange.com/a/552191/123914>
trim_file() {
  perl -0777 -pe 's/^\n+|\n\K\n+$//g'
}

cargo run --example narrow -p portable-pty $TARGET_DIR/debug/onlyterm --help | $TARGET_DIR/debug/strip-ansi-escapes | trim_file > docs/examples/cmd-synopsis-onlyterm--help.txt

for cmd in start serial connect ls-fonts show-keys imgcat set-working-directory record replay  ; do
  fname="docs/examples/cmd-synopsis-onlyterm-${cmd}--help.txt"
  cargo run --example narrow -p portable-pty $TARGET_DIR/debug/onlyterm $cmd --help | $TARGET_DIR/debug/strip-ansi-escapes | trim_file > $fname
done

for cmd in \
    activate-pane \
    activate-pane-direction \
    adjust-pane-size \
    activate-tab \
    get-pane-direction \
    get-text \
    kill-pane \
    list \
    list-clients \
    move-pane-to-new-tab \
    rename-workspace \
    send-text \
    set-tab-title \
    set-window-title \
    spawn \
    split-pane \
    zoom-pane \
    ; do
  fname="docs/examples/cmd-synopsis-onlyterm-cli-${cmd}--help.txt"
  cargo run --example narrow -p portable-pty $TARGET_DIR/debug/onlyterm cli $cmd --help | $TARGET_DIR/debug/strip-ansi-escapes | trim_file > $fname
done
