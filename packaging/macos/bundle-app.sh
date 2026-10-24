#!/usr/bin/env bash
set -euo pipefail

VER="${ALPACA_VERSION:-0.1.0}"
APP="alpaca-code.app"
BIN="target/release/alpaca-code-rs"
BREW="$(brew --prefix)"
FW="$APP/Contents/Frameworks"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS/bin" "$APP/Contents/Resources" "$FW" \
         "$APP/Contents/share/fonts" "$APP/Contents/etc/fonts" \
         "$APP/Contents/share/pixbuf-loaders" "$APP/Contents/share/glib-2.0" \
         "$APP/Contents/share/icons/hicolor"

cp "$BIN" "$APP/Contents/MacOS/bin/"
iconutil -c icns packaging/macos/alpaca.iconset -o "$APP/Contents/Resources/alpaca.icns"
sed "s|@VER@|$VER|g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
cp packaging/macos/launch.sh "$APP/Contents/MacOS/alpaca-code"
chmod +x "$APP/Contents/MacOS/alpaca-code"
cp packaging/fonts/DejaVuSans.ttf packaging/fonts/DejaVuSans-Bold.ttf \
   packaging/fonts/DejaVuSansMono.ttf packaging/fonts/DejaVuSansMono-Bold.ttf \
   "$APP/Contents/share/fonts/"
cp packaging/macos/fonts.conf "$APP/Contents/etc/fonts/fonts.conf"
cp -R "$BREW/share/glib-2.0/schemas" "$APP/Contents/share/glib-2.0/"
for s in 16 32 48 64 128 256; do
    mkdir -p "$APP/Contents/share/icons/hicolor/${s}x${s}/apps"
    cp "assets/icons/io.alpaca.rs-$s.png" \
       "$APP/Contents/share/icons/hicolor/${s}x${s}/apps/io.alpaca.rs.png"
done

LD="$APP/Contents/share/pixbuf-loaders"
for so in "$BREW"/lib/gdk-pixbuf-2.0/2.10.0/loaders/*.so; do
    cp -L "$(readlink -f "$so")" "$LD/"
done
( cd "$LD" &&
  "$BREW/bin/gdk-pixbuf-query-loaders" "$PWD"/*.so 2>/dev/null |
  sed "s|$PWD|@@ROOT@@/share/pixbuf-loaders|g" ) > "$APP/Contents/share/pixbuf-loaders.cache"

queue=("$BIN" "$LD"/*.so)
i=0
while [ "$i" -lt "${#queue[@]}" ]; do
    f="${queue[$i]}"; i=$((i + 1))
    deps=$(/usr/bin/otool -L "$f" | tail -n +2 | sed 's/(.*)$//' | awk '{print $1}')
    [ -n "$deps" ] || continue
    rps=()
    while IFS= read -r rp; do rps+=("$rp"); done < <(
        /usr/bin/otool -l "$f" | awk '/LC_RPATH/{getline; print $2}'
    )
    for dep in $deps; do
        src=""
        case "$dep" in
            /usr/lib/*|/System/*) continue ;;
            @rpath/*)
                base="${dep#@rpath/}"
                for rp in ${rps[@]+"${rps[@]}"}; do
                    [ -e "$rp/$base" ] && { src="$rp/$base"; break; }
                done
                [ -n "$src" ] || { echo "UNRESOLVED @rpath dep $base (in $f)" >&2; exit 1; } ;;
            /*) src="$dep" ;;
            *) continue ;;
        esac
        src="$(readlink -f "$src")"
        name="$(basename "$src")"
        if [ ! -e "$FW/$name" ]; then
            cp -L "$src" "$FW/$name"
            chmod +w "$FW/$name"
            /usr/bin/install_name_tool -id "@executable_path/../Frameworks/$name" "$FW/$name" 2>/dev/null ||
                true
            queue+=("$FW/$name")
        fi
        /usr/bin/install_name_tool -change "$dep" "@executable_path/../Frameworks/$name" "$f"
    done
done

find "$APP/Contents/MacOS/bin" "$FW" -type f -print0 | xargs -0 codesign --remove-signature || true
find "$APP/Contents/MacOS/bin" "$FW" -type f -print0 | xargs -0 codesign -f -s -

echo "bundle ok: $(du -sh "$APP")"