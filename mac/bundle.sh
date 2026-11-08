#!/bin/sh
# Builds the release: dist/JBL Headphones.app (universal, ad-hoc signed, with devices/ and locales/
# inside) and dist/jbl-headphones-mac.zip (the .app at its top level, what the app updates from).
# Usage: mac/bundle.sh <version>   (from anywhere; CI runs it from the repo root)
set -e
version="${1:?usage: mac/bundle.sh <version>}"
root="$(cd "$(dirname "$0")/.." && pwd)"
app="$root/dist/JBL Headphones.app"
build="$root/mac/.build"
rm -rf "$app" "$root/dist/jbl-headphones-mac.zip"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/pt-BR.lproj" "$build"

for arch in arm64 x86_64; do
    swiftc -O -swift-version 5 -target "$arch-apple-macos15" "$root"/mac/*.swift -o "$build/jbl-headphones-$arch"
done
lipo -create "$build/jbl-headphones-arm64" "$build/jbl-headphones-x86_64" -output "$app/Contents/MacOS/jbl-headphones"
cp -R "$root/devices" "$root/locales" "$app/Contents/Resources/"

# Bluetooth: without the usage text, macOS kills a bundled app the moment it asks
bt="Reads the battery and changes the settings of your JBL headphones."
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>JBL Headphones</string>
    <key>CFBundleDisplayName</key><string>JBL Headphones</string>
    <key>CFBundleIdentifier</key><string>io.github.edsuuu.jbl-headphones</string>
    <key>CFBundleExecutable</key><string>jbl-headphones</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>CFBundleDevelopmentRegion</key><string>en</string>
    <key>LSMinimumSystemVersion</key><string>15.0</string>
    <key>LSUIElement</key><true/>
    <key>NSBluetoothAlwaysUsageDescription</key><string>$bt</string>
</dict>
</plist>
EOF
echo '"NSBluetoothAlwaysUsageDescription" = "Lê a bateria e muda os ajustes dos seus fones JBL.";' > "$app/Contents/Resources/pt-BR.lproj/InfoPlist.strings"

codesign -s - --force --deep "$app"
cd "$root/dist"
ditto -c -k --keepParent "JBL Headphones.app" jbl-headphones-mac.zip
echo "$app"
echo "$root/dist/jbl-headphones-mac.zip"
