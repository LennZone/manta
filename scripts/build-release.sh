#!/usr/bin/env bash
# Builds a distributable Manta release: a hardened, universal (arm64 + x86_64)
# binary in Manta.app, code-signed, optionally notarized, packed into
# dist/Manta-<version>.dmg with a drag-to-Applications layout.
#
#   scripts/build-release.sh
#
# Environment (all optional):
#   MANTA_SIGN_IDENTITY   codesign identity, e.g. "Developer ID Application: Name (TEAMID)".
#                         Unset: the self-signed "Manta Code Signing" identity from the login
#                         keychain if present (stable signature, so macOS keeps granted
#                         permissions across updates), else ad-hoc.
#   MANTA_SELF_IDENTITY   name of that self-signed identity (default: "Manta Code Signing")
#   MANTA_NOTARY_PROFILE  keychain profile for `xcrun notarytool` (see docs/development.md). Unset: no notarization.
#   MANTA_BUNDLE_ID       CFBundleIdentifier (default: com.manta-filemanager.manta)
#   MANTA_ARCHS           "arm64" (default; Manta supports Apple silicon only), "universal", "x86_64" or "native"
#   MANTA_MIN_MACOS       LSMinimumSystemVersion / deployment target (default: 11.0)
#   MANTA_DMG_LAYOUT      0 = skip the Finder window layout (plain DMG; e.g. on CI)
#   MANTA_DIST_DIR        output folder (default: <repo>/dist)
#   CARGO_TARGET_DIR      cargo target dir (default: <repo>/target/dist, kept apart from
#                         dev builds because of the release-only RUSTFLAGS)
#   MANTA_UPDATE_ZIP      1 = also write dist/Manta-<version>.zip (ditto -c -k --keepParent of
#                         the signed app) for the auto-updater (see docs/updates.md)
#   MANTA_UPDATE_URL      update feed URL (default: https://manta-filemanager.com/updates/update.json):
#                         written into Info.plist as MantaUpdateFeedURL and compiled in
#   MANTA_UPDATE_PUBKEY   base64 update public key; default packaging/update-public-key.txt
#   MANTA_DMG             0 = skip the DMG (e.g. update ZIP only)
#   MANTA_VERSION         override the version in Info.plist and file names (testing update
#                         flows; normally bump the version in Cargo.toml instead)
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

say()  { printf '\033[1m==> %s\033[0m\n' "$*"; }
warn() { printf '\033[33mwarning:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = Darwin ] || die "macOS only (codesign, hdiutil, lipo)"
for tool in cargo rustup lipo codesign hdiutil plutil; do
    command -v "$tool" >/dev/null || die "$tool not found"
done

# From Cargo.toml itself (cargo pkgid reads Cargo.lock, which lags behind a
# freshly bumped version until the next build).
VERSION=$(cargo metadata --no-deps --format-version 1 --offline 2>/dev/null \
    | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="manta"))')
[ -n "$VERSION" ] || die "could not read the version of the manta package"
if [ -n "${MANTA_VERSION:-}" ]; then
    [[ "$MANTA_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] \
        || die "MANTA_VERSION must be a semantic version like 1.2.3 or 1.2.3-beta.1"
    warn "version override: $MANTA_VERSION (Cargo.toml says $VERSION)"
    VERSION=$MANTA_VERSION
fi
UPDATE_URL=${MANTA_UPDATE_URL-https://manta-filemanager.com/updates/update.json}
if [ -n "$UPDATE_URL" ]; then
    case "$UPDATE_URL" in
        https://*|http://127.0.0.1[:/]*|http://localhost[:/]*) ;;
        *) die "MANTA_UPDATE_URL must start with https:// (got $UPDATE_URL)" ;;
    esac
    export MANTA_UPDATE_URL="$UPDATE_URL"
fi
if [ -n "${MANTA_UPDATE_PUBKEY:-}" ] || [ -s "$ROOT/packaging/update-public-key.txt" ]; then
    UPDATE_KEY=yes
else
    UPDATE_KEY=no
fi
BUILD=${MANTA_BUILD_NUMBER:-$VERSION}
BUNDLE_ID=${MANTA_BUNDLE_ID:-com.manta-filemanager.manta}
MIN_MACOS=${MANTA_MIN_MACOS:-11.0}
DIST=${MANTA_DIST_DIR:-$ROOT/dist}
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$ROOT/target/dist}
export MACOSX_DEPLOYMENT_TARGET=$MIN_MACOS
APP="$DIST/Manta.app"
DMG="$DIST/Manta-$VERSION.dmg"
ICNS="$ROOT/packaging/icon/AppIcon.icns"
ZIP="$DIST/Manta-$VERSION.zip"
MAKE_DMG=${MANTA_DMG:-1}
MAKE_ZIP=${MANTA_UPDATE_ZIP:-0}
[ "$MAKE_DMG" != 0 ] || [ "$MAKE_ZIP" = 1 ] || die "MANTA_DMG=0 needs MANTA_UPDATE_ZIP=1 (nothing to build otherwise)"

say "Manta $VERSION ($BUNDLE_ID), macOS $MIN_MACOS+"

# The licence notices of every linked crate ship with the app (and are shown
# in its help window); they must match Cargo.lock.
python3 "$ROOT/scripts/third-party-licenses.py" --check \
    || die "docs/third-party-licenses.md is stale or lists a copyleft crate (run scripts/third-party-licenses.py, review, commit)"

# ---------------------------------------------------------------- build
# Strip local paths (home folder, cargo registry, toolchain, checkout) from panic
# messages, file!() and dbg!() output in the binary. Release-only, so dev builds
# and tests keep real paths. rustc applies the LAST matching mapping first, so the
# catch-all $HOME comes first and the more specific prefixes after it.
CARGO_HOME_DIR=${CARGO_HOME:-$HOME/.cargo}
RUSTUP_HOME_DIR=${RUSTUP_HOME:-$HOME/.rustup}
remaps=(
    "--remap-path-prefix=$HOME=~"
    "--remap-path-prefix=$RUSTUP_HOME_DIR=/rustup"
    "--remap-path-prefix=$CARGO_HOME_DIR=/cargo"
    "--remap-path-prefix=$ROOT=/manta"
    "--remap-path-prefix=$CARGO_TARGET_DIR=/target"
)
# Unit separator between flags, so paths with spaces survive.
CARGO_ENCODED_RUSTFLAGS=$(IFS=$'\x1f'; echo "${remaps[*]}")
export CARGO_ENCODED_RUSTFLAGS
unset RUSTFLAGS

case "${MANTA_ARCHS:-arm64}" in
    universal) TARGETS=(aarch64-apple-darwin x86_64-apple-darwin) ;;
    arm64)     TARGETS=(aarch64-apple-darwin) ;;
    x86_64)    TARGETS=(x86_64-apple-darwin) ;;
    native)    TARGETS=("$(rustc -vV | sed -n 's/^host: //p')") ;;
    *)         die "MANTA_ARCHS must be universal, arm64, x86_64 or native" ;;
esac

BINS=()
for t in "${TARGETS[@]}"; do
    if ! rustup target list --installed | grep -qx "$t"; then
        say "Installing Rust target $t"
        if ! rustup target add "$t"; then
            [ ${#TARGETS[@]} -gt 1 ] || die "could not install target $t"
            warn "could not install $t; building without it"
            continue
        fi
    fi
    say "Building $t (release, fat LTO; this takes a few minutes)"
    if cargo build --release -p manta --target "$t"; then
        # Copy (the original stays untouched for cargo) and scrub C-level build paths.
        bin="$CARGO_TARGET_DIR/$t/release/manta-dist"
        cp "$CARGO_TARGET_DIR/$t/release/manta" "$bin"
        n=$(python3 "$ROOT/scripts/scrub-build-paths.py" "$bin" \
            "$CARGO_TARGET_DIR" "$(cd "$CARGO_TARGET_DIR" && pwd -P)")
        [ "$n" = 0 ] || echo "    scrubbed $n build path(s) from C dependencies"
        BINS+=("$bin")
    else
        [ ${#TARGETS[@]} -gt 1 ] || die "build for $t failed"
        warn "build for $t failed; the app will not include $t"
    fi
done
[ ${#BINS[@]} -gt 0 ] || die "no binary was built"

# ---------------------------------------------------------------- bundle
say "Assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
if [ ${#BINS[@]} -gt 1 ]; then
    lipo -create -output "$APP/Contents/MacOS/manta" "${BINS[@]}"
else
    cp "${BINS[0]}" "$APP/Contents/MacOS/manta"
fi
ARCHS=$(lipo -archs "$APP/Contents/MacOS/manta")
# Guard: nothing that identifies the build machine may remain in the binary.
leaks=$(strings -a "$APP/Contents/MacOS/manta" | grep -F -e "$HOME" -e "$ROOT" -e "$CARGO_TARGET_DIR" \
    -e "/Users/$(id -un)" | head -n 5 || true)
[ -z "$leaks" ] || die "local paths left in the binary:
$leaks"
[ -f "$ICNS" ] || die "$ICNS missing (python3 packaging/icon/make_icons.py)"
cp "$ICNS" "$APP/Contents/Resources/AppIcon.icns"
cp "$ROOT/LICENSE" "$APP/Contents/Resources/LICENSE.txt"
cp "$ROOT/docs/third-party-licenses.md" "$APP/Contents/Resources/ThirdPartyLicenses.md"
cp -R "$ROOT/packaging/macos/en.lproj" "$ROOT/packaging/macos/de.lproj" "$APP/Contents/Resources/"
sed -e "s|@VERSION@|$VERSION|g" -e "s|@BUILD@|$BUILD|g" -e "s|@BUNDLE_ID@|$BUNDLE_ID|g" \
    -e "s|@MIN_MACOS@|$MIN_MACOS|g" -e "s|@YEAR@|$(date +%Y)|g" \
    "$ROOT/packaging/macos/Info.plist.in" > "$APP/Contents/Info.plist"
if [ -n "$UPDATE_URL" ]; then
    plutil -insert MantaUpdateFeedURL -string "$UPDATE_URL" "$APP/Contents/Info.plist"
    echo "    update feed: $UPDATE_URL (update key: $UPDATE_KEY)"
elif [ "$MAKE_ZIP" = 1 ]; then
    warn "MANTA_UPDATE_URL is not set: this build will not look for updates by itself"
fi
plutil -lint -s "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

# ---------------------------------------------------------------- sign
ENTITLEMENTS="$ROOT/packaging/entitlements.plist"
SELF_IDENTITY="${MANTA_SELF_IDENTITY:-Manta Code Signing}"
if [ -n "${MANTA_SIGN_IDENTITY:-}" ]; then
    say "Signing with \"$MANTA_SIGN_IDENTITY\" (hardened runtime, secure timestamp)"
    codesign --force --options runtime --timestamp --entitlements "$ENTITLEMENTS" \
        --sign "$MANTA_SIGN_IDENTITY" "$APP"
elif security find-certificate -c "$SELF_IDENTITY" >/dev/null 2>&1; then
    # Self-signed: no Apple timestamp or hardened runtime, but the designated
    # requirement (identifier + certificate) stays the same across releases,
    # so TCC and Keychain grants survive updates.
    say "Signing with self-signed \"$SELF_IDENTITY\""
    codesign --force --timestamp=none --entitlements "$ENTITLEMENTS" --sign "$SELF_IDENTITY" "$APP"
    codesign -d -r- "$APP" 2>&1 | grep -q 'certificate root' || die "designated requirement does not pin the certificate"
else
    warn "Ad-hoc signing: macOS will ask for permissions again after each update (create \"$SELF_IDENTITY\" or set MANTA_SIGN_IDENTITY)"
    codesign --force --entitlements "$ENTITLEMENTS" --sign - "$APP"
fi
codesign --verify --deep --strict "$APP"

# ---------------------------------------------------------------- update zip
# The auto-updater downloads this (see docs/updates.md). ditto keeps the code
# signature, symlinks and extended attributes intact; plain `zip` does not.
make_update_zip() {
    mkdir -p "$DIST"
    rm -f "$ZIP"
    ditto -c -k --keepParent "$APP" "$ZIP"
}

WORK=$(mktemp -d "${TMPDIR:-/tmp}/manta-dmg.XXXXXX")
MNT=""
cleanup() {
    if [ -n "$MNT" ] && [ -d "$MNT" ]; then hdiutil detach "$MNT" -force -quiet || true; fi
    rm -rf "$WORK"
}
trap cleanup EXIT

if [ "$MAKE_DMG" = 0 ]; then
    say "Skipping the DMG (MANTA_DMG=0)"
    make_update_zip
    if [ -n "${MANTA_NOTARY_PROFILE:-}" ]; then
        [ -n "${MANTA_SIGN_IDENTITY:-}" ] || die "notarization needs a Developer ID signature (MANTA_SIGN_IDENTITY)"
        say "Notarizing the update ZIP (keychain profile \"$MANTA_NOTARY_PROFILE\")"
        out=$(xcrun notarytool submit "$ZIP" --keychain-profile "$MANTA_NOTARY_PROFILE" --wait 2>&1) || true
        echo "$out"
        grep -q "status: Accepted" <<<"$out" || die "notarization failed"
        xcrun stapler staple "$APP"
        make_update_zip
    fi
    size=$(du -h "$ZIP" | cut -f1 | tr -d ' ')
    say "Done: $ZIP ($size; sha256 $(shasum -a 256 "$ZIP" | cut -d' ' -f1))"
    exit 0
fi

# ---------------------------------------------------------------- dmg
say "Creating $DMG"

# Preferred: dmgbuild writes the window layout (.DS_Store) itself, so it
# needs no Finder automation permission and works on CI. It lives in a
# venv under target/ (installed once; MANTA_DMGBUILD=/path/to/dmgbuild
# to use your own, MANTA_DMGBUILD=0 to skip it).
layout_ok=0
DMGBUILD=${MANTA_DMGBUILD:-}
if [ "${MANTA_DMG_LAYOUT:-1}" != 0 ] && [ "$DMGBUILD" != 0 ]; then
    if [ -z "$DMGBUILD" ]; then
        DMGBUILD="$ROOT/target/dmgbuild-venv/bin/dmgbuild"
        if [ ! -x "$DMGBUILD" ]; then
            say "Installing dmgbuild (once) into target/dmgbuild-venv"
            python3 -m venv "$ROOT/target/dmgbuild-venv" >/dev/null 2>&1 \
                && "$ROOT/target/dmgbuild-venv/bin/pip" install -q "dmgbuild==1.6.7" >/dev/null 2>&1 \
                || warn "could not install dmgbuild; falling back to Finder scripting"
        fi
    fi
    if [ -x "$DMGBUILD" ]; then
        mkdir -p "$DIST"
        rm -f "$DMG"
        if "$DMGBUILD" -s "$ROOT/packaging/dmg/dmgbuild_settings.py" \
            -D app="$APP" -D bg="$ROOT/packaging/dmg/background.png" \
            -D volicon="$ROOT/packaging/icon/AppIcon.icns" \
            "Manta" "$DMG" >"$WORK/dmgbuild.log" 2>&1; then
            layout_ok=2
        else
            warn "dmgbuild failed ($(tail -1 "$WORK/dmgbuild.log")); falling back to Finder scripting"
            rm -f "$DMG"
        fi
    fi
fi

if [ $layout_ok = 0 ]; then
STAGE="$WORK/stage"
mkdir -p "$STAGE/.background"
ditto "$APP" "$STAGE/Manta.app"
ln -s /Applications "$STAGE/Applications"
cp "$ROOT/packaging/dmg/background.tiff" "$STAGE/.background/background.tiff"

VOL="Manta"
RW="$WORK/rw.dmg"
hdiutil create -quiet -volname "$VOL" -srcfolder "$STAGE" -fs HFS+ -format UDRW -ov "$RW"

layout_ok=0
if [ "${MANTA_DMG_LAYOUT:-1}" != 0 ]; then
    if [ -d "/Volumes/$VOL" ]; then
        warn "a volume named $VOL is already mounted; skipping the Finder layout"
    else
        MNT="/Volumes/$VOL"
        hdiutil attach -quiet -readwrite -noverify -noautoopen "$RW"
        # Window: 660x400 content (the size of the background), icons 128 pt.
        # Finder may ask once for permission to be automated by your terminal; if
        # that is denied or times out, the DMG is still fine, just without layout.
        osascript >/dev/null 2>"$WORK/osa.log" <<OSA &
tell application "Finder"
    tell disk "$VOL"
        open
        set current view of container window to icon view
        set toolbar visible of container window to false
        set statusbar visible of container window to false
        set the bounds of container window to {200, 120, 860, 548}
        set opts to the icon view options of container window
        set arrangement of opts to not arranged
        set icon size of opts to 128
        set text size of opts to 13
        set background picture of opts to file ".background:background.tiff"
        set position of item "Manta.app" of container window to {165, 190}
        set position of item "Applications" of container window to {495, 190}
        close
        open
        update without registering applications
        delay 1
        close
    end tell
end tell
OSA
        osa_pid=$!
        for _ in $(seq 1 60); do kill -0 "$osa_pid" 2>/dev/null || break; sleep 1; done
        if kill -0 "$osa_pid" 2>/dev/null; then
            kill "$osa_pid" 2>/dev/null || true
            warn "Finder scripting timed out (allow your terminal under System Settings > Privacy & Security > Automation > Finder, then rerun); creating a plain DMG"
        elif wait "$osa_pid"; then
            layout_ok=1
        else
            warn "Finder scripting not allowed ($(tr '\n' ' ' < "$WORK/osa.log")); creating a plain DMG"
        fi
        chmod -Rf go-w "$MNT" 2>/dev/null || true
        sync
        for _ in 1 2 3 4 5; do hdiutil detach -quiet "$MNT" && break; sleep 2; done
        [ -d "$MNT" ] && hdiutil detach -quiet -force "$MNT"
        MNT=""
    fi
fi

mkdir -p "$DIST"
rm -f "$DMG"
hdiutil convert -quiet "$RW" -format UDZO -imagekey zlib-level=9 -o "$DMG"
fi

if [ -n "${MANTA_SIGN_IDENTITY:-}" ]; then
    codesign --force --timestamp --sign "$MANTA_SIGN_IDENTITY" "$DMG"
fi

# ---------------------------------------------------------------- notarize
if [ -n "${MANTA_NOTARY_PROFILE:-}" ]; then
    [ -n "${MANTA_SIGN_IDENTITY:-}" ] || die "notarization needs a Developer ID signature (MANTA_SIGN_IDENTITY)"
    say "Notarizing (keychain profile \"$MANTA_NOTARY_PROFILE\"); this usually takes a few minutes"
    out=$(xcrun notarytool submit "$DMG" --keychain-profile "$MANTA_NOTARY_PROFILE" --wait 2>&1) || true
    echo "$out"
    if ! grep -q "status: Accepted" <<<"$out"; then
        id=$(sed -n 's/^ *id: //p' <<<"$out" | head -n 1)
        die "notarization failed; details: xcrun notarytool log ${id:-<id>} --keychain-profile \"$MANTA_NOTARY_PROFILE\""
    fi
    xcrun stapler staple "$DMG"
    xcrun stapler staple "$APP" || warn "could not staple the ticket to $APP (the DMG is stapled)"
    spctl --assess --type open --context context:primary-signature -v "$DMG" || warn "spctl rejected the DMG"
else
    say "Skipping notarization (set MANTA_NOTARY_PROFILE to notarize and staple)"
fi

# After notarization, so the update ZIP carries the stapled app.
if [ "$MAKE_ZIP" = 1 ]; then
    make_update_zip
    say "Update ZIP: $ZIP ($(du -h "$ZIP" | cut -f1 | tr -d ' '); sha256 $(shasum -a 256 "$ZIP" | cut -d' ' -f1))"
fi

size=$(du -h "$DMG" | cut -f1 | tr -d ' ')
echo
case $layout_ok in
    2) layout_name=dmgbuild ;;
    1) layout_name=Finder ;;
    *) layout_name=plain ;;
esac
say "Done: $DMG ($size; $ARCHS; layout: $layout_name)"
