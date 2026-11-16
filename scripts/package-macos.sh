#!/usr/bin/env bash
# Builds a self-contained DevSticky.app (GTK libraries and data bundled inside)
# and wraps it in dist/DevSticky-<version>-<arch>.dmg.
#
# Needs Homebrew:
#   brew install gtk4 libadwaita gtksourceview5 adwaita-icon-theme librsvg dylibbundler pkg-config
#
# The app is ad-hoc signed, not notarized: on first launch users right-click it
# and choose Open (or run: xattr -dr com.apple.quarantine /Applications/DevSticky.app).
set -euo pipefail
cd "$(dirname "$0")/.."

APP_ID=io.github.technophilix.DevSticky
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
ARCH=$(uname -m)
BREW=$(brew --prefix)
# Homebrew bottles target the macOS version they were built on.
MIN_MACOS=$(sw_vers -productVersion | cut -d. -f1).0

OUT=target/macos
APP=$OUT/DevSticky.app
CONTENTS=$APP/Contents
RES=$CONTENTS/Resources
SHARE=$RES/share
PIXBUF_DIR=lib/gdk-pixbuf-2.0/2.10.0
LOADERS=$RES/$PIXBUF_DIR/loaders

echo "==> Building DevSticky $VERSION ($ARCH)"
cargo build --release --locked

rm -rf "$OUT"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Frameworks" "$SHARE" "$LOADERS" dist
cp target/release/devsticky "$CONTENTS/MacOS/devsticky"

echo "==> Info.plist"
cat > "$CONTENTS/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>              <string>DevSticky</string>
    <key>CFBundleDisplayName</key>       <string>DevSticky</string>
    <key>CFBundleIdentifier</key>        <string>$APP_ID</string>
    <key>CFBundleExecutable</key>        <string>devsticky</string>
    <key>CFBundleIconFile</key>          <string>DevSticky</string>
    <key>CFBundlePackageType</key>       <string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key>           <string>$VERSION</string>
    <key>LSMinimumSystemVersion</key>    <string>$MIN_MACOS</string>
    <key>LSApplicationCategoryType</key> <string>public.app-category.developer-tools</string>
    <key>NSHighResolutionCapable</key>   <true/>
    <!-- Menu-bar app: no Dock icon. -->
    <key>LSUIElement</key>               <true/>
    <key>NSHumanReadableCopyright</key>  <string>© 2026 technophilix, MIT License</string>
</dict>
</plist>
EOF

echo "==> App icon"
ICONSET=$OUT/DevSticky.iconset
mkdir -p "$ICONSET"
ICONS=data/icons/hicolor
for s in 16 32 128 256 512; do
    cp "$ICONS/${s}x${s}/apps/$APP_ID.png" "$ICONSET/icon_${s}x${s}.png"
done
cp "$ICONS/32x32/apps/$APP_ID.png"   "$ICONSET/icon_16x16@2x.png"
cp "$ICONS/64x64/apps/$APP_ID.png"   "$ICONSET/icon_32x32@2x.png"
cp "$ICONS/256x256/apps/$APP_ID.png" "$ICONSET/icon_128x128@2x.png"
cp "$ICONS/512x512/apps/$APP_ID.png" "$ICONSET/icon_256x256@2x.png"
iconutil -c icns "$ICONSET" -o "$RES/DevSticky.icns"

echo "==> GTK data"
mkdir -p "$SHARE/glib-2.0/schemas" "$SHARE/icons/hicolor"
cp "$BREW"/share/glib-2.0/schemas/org.gtk.gtk4.*.gschema.xml "$SHARE/glib-2.0/schemas/"
glib-compile-schemas "$SHARE/glib-2.0/schemas"
# -L: Homebrew's share/ is full of symlinks into its Cellar; copy the real files.
# Cursors are skipped: GTK uses the native cursors on macOS.
rsync -aL --exclude cursors "$BREW/share/icons/Adwaita/" "$SHARE/icons/Adwaita/"
cp -L "$BREW/share/icons/hicolor/index.theme" "$SHARE/icons/hicolor/"
rsync -aL "$BREW/share/gtksourceview-5/" "$SHARE/gtksourceview-5/"

# SVG loader, for the symbolic icons. Its cache is filled in at startup
# (src/macos_bundle.rs) because it needs absolute paths.
cp "$BREW/$PIXBUF_DIR/loaders/libpixbufloader_svg.so" "$LOADERS/"

echo "==> Bundling libraries"
dylibbundler --overwrite-files --bundle-deps --create-dir \
    --fix-file "$CONTENTS/MacOS/devsticky" \
    --fix-file "$LOADERS/libpixbufloader_svg.so" \
    --search-path "$BREW/lib" \
    --dest-dir "$CONTENTS/Frameworks" \
    --install-path @executable_path/../Frameworks/ </dev/null >/dev/null

# dylibbundler can add the same LC_RPATH twice, which dyld refuses to load.
for f in "$CONTENTS/MacOS/devsticky" "$CONTENTS"/Frameworks/*.dylib "$LOADERS"/*.so; do
    for rp in $(otool -l "$f" | awk '/cmd LC_RPATH/{getline; getline; print $2}' | sort | uniq -d); do
        install_name_tool -delete_rpath "$rp" "$f" 2>/dev/null
    done
done

install_name_tool -id libpixbufloader_svg.so "$LOADERS/libpixbufloader_svg.so" 2>/dev/null

# The SVG loader's entry from Homebrew's cache, with its path made relocatable.
awk -v RS= -v ORS='\n\n' '/libpixbufloader_svg\.so/' "$BREW/$PIXBUF_DIR/loaders.cache" \
    | sed "s|\"[^\"]*/libpixbufloader_svg\.so\"|\"@RESOURCES@/$PIXBUF_DIR/loaders/libpixbufloader_svg.so\"|" \
    > "$RES/$PIXBUF_DIR/loaders.cache.in"
grep -q '@RESOURCES@' "$RES/$PIXBUF_DIR/loaders.cache.in" \
    || { echo "SVG loader missing from $BREW/$PIXBUF_DIR/loaders.cache" >&2; exit 1; }

# Fail if anything still points at Homebrew.
if otool -L "$CONTENTS/MacOS/devsticky" "$CONTENTS"/Frameworks/*.dylib "$LOADERS"/*.so \
        | grep -q "$BREW"; then
    echo "Some libraries still reference $BREW:" >&2
    otool -L "$CONTENTS/MacOS/devsticky" "$CONTENTS"/Frameworks/*.dylib "$LOADERS"/*.so | grep "$BREW" >&2
    exit 1
fi

echo "==> Signing (ad-hoc)"
find "$CONTENTS/Frameworks" "$LOADERS" -type f -exec codesign --force --sign - {} \;
codesign --force --sign - "$APP"
codesign --verify --strict "$APP"
if [ -n "$(find "$APP" -type l)" ]; then
    echo "Symlinks left in the bundle:" >&2; find "$APP" -type l >&2; exit 1
fi

echo "==> Disk image"
DMG=dist/DevSticky-$VERSION-$ARCH.dmg
STAGE=$OUT/dmg
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
hdiutil create -quiet -volname "DevSticky $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG"

echo "Built $APP and $DMG ($(du -h "$DMG" | cut -f1))"
