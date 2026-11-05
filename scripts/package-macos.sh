#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
profile="${1:-debug}"
identity="${AXOLOTL_SIGNING_IDENTITY:--}"
entitlements="${AXOLOTL_ENTITLEMENTS:-}"
provisioning="${AXOLOTL_PROVISIONING_PROFILE:-}"
bundle_id="${AXOLOTL_BUNDLE_ID:-dev.axolotl.prototype}"
# Check opt-in signing inputs before spending time building. Never attach the
# restricted entitlement to an ad-hoc signature or leave an old profile behind.
if [ "$identity" != '-' ]; then
  if [ ! -f "$entitlements" ] || [ ! -f "$provisioning" ]; then
    echo 'Provisioned signing requires AXOLOTL_ENTITLEMENTS and AXOLOTL_PROVISIONING_PROFILE files. See docs/passkeys-macos.md.' >&2
    exit 2
  fi
  plutil -lint "$entitlements" >/dev/null
  if [ "$(/usr/libexec/PlistBuddy -c 'Print :com.apple.developer.web-browser.public-key-credential' "$entitlements")" != true ]; then
    echo 'The browser passkey entitlement must be true.' >&2
    exit 2
  fi
elif [ -n "$entitlements" ] || [ -n "$provisioning" ]; then
  echo 'Passkey signing inputs require AXOLOTL_SIGNING_IDENTITY (not ad-hoc signing).' >&2
  exit 2
fi
case "$profile" in
  debug) cargo build -p axolotl --locked ;;
  release) cargo build -p axolotl --locked --release ;;
  *) echo 'Usage: scripts/package-macos.sh [debug|release]' >&2; exit 2 ;;
esac
bundle="target/$profile/Axolotl.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources/licenses/ogv"
cp crates/desktop/assets/vendor/ogv/COPYING crates/desktop/assets/vendor/ogv/LICENSE-vpx.txt \
  crates/desktop/assets/vendor/ogv/PATENTS-vpx.txt crates/desktop/assets/vendor/ogv/README.md \
  "$bundle/Contents/Resources/licenses/ogv/"
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
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $bundle_id" "$bundle/Contents/Info.plist"
rm -f "$bundle/Contents/embedded.provisionprofile"
if [ "$identity" = '-' ]; then
  codesign --force --sign - "$bundle"
else
  cp "$provisioning" "$bundle/Contents/embedded.provisionprofile"
  codesign --force --options runtime --timestamp --sign "$identity" --entitlements "$entitlements" "$bundle"
fi
codesign --verify --strict "$bundle"
printf 'Built %s\n' "$bundle"
