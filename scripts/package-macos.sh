#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
profile="${1:-debug}"
case "$profile" in
  debug) cargo build -p axolotl --locked ;;
  release) cargo build -p axolotl --locked --release ;;
  *) echo 'Usage: scripts/package-macos.sh [debug|release]' >&2; exit 2 ;;
esac
bundle="target/$profile/Axolotl.app"
mkdir -p "$bundle/Contents/MacOS"
rm -f "$bundle/Contents/MacOS/axolotl"
cp "target/$profile/axolotl" "$bundle/Contents/MacOS/Axolotl"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>Axolotl</string>
<key>CFBundleIdentifier</key><string>dev.axolotl.prototype</string>
<key>CFBundleName</key><string>Axolotl</string>
<key>CFBundleDisplayName</key><string>Axolotl</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSAppTransportSecurity</key><dict><key>NSAllowsArbitraryLoadsInWebContent</key><true/></dict>
</dict></plist>
PLIST
codesign --force --sign - "$bundle"
printf 'Built %s\n' "$bundle"
