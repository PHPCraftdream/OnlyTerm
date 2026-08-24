#!/bin/bash
set -x
name="$1"

notes=$(cat <<EOT
OnlyTerm $name -- a Windows-only fork of [OnlyTerm](https://github.com/wezterm/wezterm).

Download the \`.zip\` (portable) or the \`.exe\` (installer) below; each is
published alongside a \`.sha256\` you can verify it against.
EOT
)

gh release view "$name" || gh release create --prerelease --notes "$notes" --title "$name" "$name"
