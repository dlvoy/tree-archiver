#!/usr/bin/env bash
#
# Builds the release notes for one version and prints them to stdout.
#
#   release-notes.sh <version> [changelog-path]
#
# The body is this version's section lifted out of CHANGELOG.md, followed by a
# fixed block describing the downloads. Kept as a script rather than inline in
# the workflow so the markdown does not have to survive YAML block-scalar
# indentation rules, and so it can be run locally.

set -euo pipefail

version="${1:?usage: release-notes.sh <version> [changelog-path]}"
changelog="${2:-CHANGELOG.md}"

body=""
if [ -f "$changelog" ]; then
  # Match the heading by literal prefix. A regex would need the brackets
  # escaped, and an under-escaped "\[" turns into a character class that
  # matches every "## " heading in the file.
  body="$(
    awk -v ver="$version" '
      !inside && index($0, "## [" ver "]") == 1 { inside = 1; next }
      !inside && index($0, "## " ver) == 1      { inside = 1; next }
      inside && index($0, "## ") == 1           { exit }
      # Reference-style link definitions belong to the file, not the notes.
      inside && substr($0, 1, 1) == "[" && index($0, "]: ") > 0 { next }
      inside { print }
    ' "$changelog" \
      | sed -e :a -e '/./,$!d' -e '/^\n*$/{$d;N;};/\n$/ba'
  )"
fi

if [ -z "$body" ]; then
  echo "warning: no ${changelog} section for ${version}" >&2
  body="Windows release ${version}."
fi

printf '%s\n' "$body"

cat <<EOF

### Downloads

| File | Use |
| --- | --- |
| \`TreeArchiver-${version}-x64-setup.exe\` | NSIS installer — start menu entry and uninstaller |
| \`TreeArchiver-${version}-x64-portable.exe\` | The application on its own, nothing to install |
| \`TreeArchiver-${version}-x64.msi\` | MSI, for deployment through Group Policy or Intune |

Requires 64-bit Windows 10 or 11 and the WebView2 runtime, which ships with
both. To verify a download against \`checksums.txt\`:

\`\`\`
certutil -hashfile <file> SHA256
\`\`\`

These builds are unsigned, so SmartScreen warns on first run — choose
**More info**, then **Run anyway**.
EOF
