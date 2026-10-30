#!/bin/sh
# Builds "WebSkin Player.app" (and a zip of it) in mac/dist. VERSION overrides the Cargo version.
# The app is ad-hoc signed, not notarized: the first launch needs Open Anyway in Privacy & Security.
set -eu

cd "$(dirname "$0")"
VERSION=${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' webskin/Cargo.toml | head -n 1)}
APP="dist/WebSkin Player.app"

cargo build --release

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Frameworks" "$APP/Contents/Resources"
cp target/release/webskin "$APP/Contents/MacOS/webskin"
cp target/release/libnowplaying_host.dylib "$APP/Contents/Frameworks/"

# The Windows app's icon, as an .icns.
ICONSET=$(mktemp -d)/app.iconset
mkdir -p "$ICONSET"
sips -s format png ../src/WebSkinPlayer/app.ico --out "$ICONSET/icon_256x256.png" > /dev/null
for size in 16 32 128; do
  sips -z $size $size "$ICONSET/icon_256x256.png" --out "$ICONSET/icon_${size}x${size}.png" > /dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/app.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>WebSkin Player</string>
  <key>CFBundleDisplayName</key><string>WebSkin Player</string>
  <key>CFBundleIdentifier</key><string>io.github.bugshotss.webskinplayer</string>
  <key>CFBundleExecutable</key><string>webskin</string>
  <key>CFBundleIconFile</key><string>app</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSAudioCaptureUsageDescription</key><string>WebSkin Player listens to what your Mac plays to draw the visualizations. Nothing is recorded or saved.</string>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>Windows Media Player Skin</string>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>LSHandlerRank</key><string>Owner</string>
      <key>LSItemContentTypes</key><array><string>io.github.bugshotss.webskinplayer.wmz</string></array>
    </dict>
  </array>
  <key>UTImportedTypeDeclarations</key>
  <array>
    <dict>
      <key>UTTypeIdentifier</key><string>io.github.bugshotss.webskinplayer.wmz</string>
      <key>UTTypeDescription</key><string>Windows Media Player Skin</string>
      <key>UTTypeConformsTo</key><array><string>public.data</string></array>
      <key>UTTypeTagSpecification</key>
      <dict><key>public.filename-extension</key><array><string>wmz</string><string>wms</string></array></dict>
    </dict>
  </array>
</dict>
</plist>
PLIST

codesign --force --sign - "$APP/Contents/Frameworks/libnowplaying_host.dylib"
codesign --force --sign - "$APP"
ditto -c -k --keepParent "$APP" "dist/WebSkinPlayer-$VERSION-mac.zip"
echo "Built $APP and dist/WebSkinPlayer-$VERSION-mac.zip"
