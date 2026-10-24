#!/bin/sh
# CFBundleExecutable: sets up runtime caches (writable dirs) from the read-only
# bundle, then execs the real binary. @@ROOT@@/@@CACHEDIR@@ placeholders from
# bundle-app.sh resolve here.
HERE="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"   # .../Contents
WORK="${TMPDIR:-/tmp}/alpaca-code-rs"
mkdir -p "$WORK"

if [ ! -f "$WORK/fonts.conf" ]; then
    mkdir -p "$WORK/fc-cache"
    sed -e "s|@@ROOT@@|$HERE|g" -e "s|@@CACHEDIR@@|$WORK/fc-cache|g" \
        "$HERE/etc/fonts/fonts.conf" > "$WORK/fonts.conf"
fi
if [ ! -f "$WORK/loaders.cache" ]; then
    sed -e "s|@@ROOT@@|$HERE|g" "$HERE/share/pixbuf-loaders.cache" > "$WORK/loaders.cache"
fi

export FONTCONFIG_FILE="$WORK/fonts.conf"
export GDK_PIXBUF_MODULE_FILE="$WORK/loaders.cache"
export XDG_DATA_DIRS="$HERE/share"
exec "$HERE/MacOS/bin/alpaca-code-rs" "$@"