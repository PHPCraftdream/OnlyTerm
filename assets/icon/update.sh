#!/bin/bash
# This script updates the icon files from the svg file.
# It assumes that the svg file is square.
set -x
cd $(git rev-parse --show-toplevel)/assets/icon

src=wezterm-icon.svg

conv_opts="-colors 256 -background none -density 300"

# the linux icon
convert $conv_opts -resize "!128x128" "$src" ../icon/terminal.png

# The Windows icon
convert $conv_opts -define icon:auto-resize=256,128,96,64,48,32,16 $src ../windows/terminal.ico

