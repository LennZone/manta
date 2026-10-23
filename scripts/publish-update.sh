#!/usr/bin/env bash
# Publishes a Manta update to a static web space (see docs/updates.md).
#
#   scripts/publish-update.sh [--dry-run] [--skip-build] [--no-dmg] [--channel stable|beta] [--keep N]
#
# 1. builds the release with the update ZIP (scripts/build-release.sh, MANTA_UPDATE_ZIP=1)
# 2. reads release-notes/<version>.en.md and .de.md (creates templates and stops if missing)
# 3. fetches the live update.json (+ .sig) and adds/replaces this version, keeping the newest N;
#    if there is no live feed yet, it starts from MANTA_UPDATE_SEED_URL if set (and mirrors
#    the ZIPs its entries need), else a new feed
# 4. signs the ZIP entry (absolute URL next to the feed) and the manifest (manta-update-sign)
# 5. uploads the ZIP(s) first, then update.json.sig + update.json, and checks the live feed
# 6. uploads the DMG to the download folder: Manta-<version>.dmg, and for stable releases
#    also Manta.dmg (the website's button) and latest.json {version, dmg, url, size, sha256,
#    published} (shown on the website)
# 7. runs an external website publisher if one is configured (MANTA_SITE_PUBLISH; the
#    website is not part of this repository); stable channel only, MANTA_PUBLISH_SITE=0
#    skips it
#
# --dry-run     everything up to step 4 (+ the download files), written to
#               dist/update-staging/, nothing uploaded; also checks the upload setup
#               (Keychain item) without logging in and prints the exact upload commands
# --skip-build  reuse dist/Manta-<version>.zip (and .dmg) from an earlier build
# --no-dmg      don't publish the DMG / latest.json (step 6)
# --setup-help  print the one-time setup commands (signing key, Keychain item) and exit
#
# Public URLs default to the official feed (https://manta-filemanager.com/updates/update.json)
# and download folder (https://manta-filemanager.com/download/). The upload destinations
# are not in the repository: they come from the environment or from the untracked
# ~/.config/manta/publish.env (see scripts/lib/publish-env.sh), e.g.
#   MANTA_UPDATE_DEST=ftp://ftp.example.com/updates/
#   MANTA_DOWNLOAD_DEST=ftp://ftp.example.com/download/
# Use the FTP host name the server's TLS certificate is issued for (which may differ
# from the website's domain); TLS is always verified.
#
# FTP credentials come from the macOS Keychain: a generic password with service
# "manta-update-ftp" and the FTP user as account. The password never appears in a
# file, in command arguments (ps) or in the output: it is read with
# `security … -w` into a variable and piped to the uploader's stdin.
#
# The FTPS upload uses manta-update-upload (crates/update-upload), which is Manta's
# own FTP backend: TLS login, data connections that resume the TLS session, and a
# clear data channel if the network resets encrypted data connections (the login
# stays encrypted; the files are Ed25519-signed). Each file goes to NAME.uploading
# and is renamed afterwards; sizes are verified.
#
# Environment (or ~/.config/manta/publish.env; MANTA_PUBLISH_ENV names another file):
#   MANTA_UPDATE_URL      public feed URL (default https://manta-filemanager.com/updates/update.json)
#   MANTA_UPDATE_DEST     upload target (required, no default)
#                           ftp://host/path/   FTPS (explicit TLS, always required)
#                           user@host:/path/   rsync/scp over SSH
#   MANTA_UPDATE_SEED_URL feed to start from when MANTA_UPDATE_URL has none yet
#                           (default empty = none)
#   MANTA_DOWNLOAD_URL    public download folder (default https://manta-filemanager.com/download/)
#   MANTA_DOWNLOAD_DEST   its upload target (required unless --no-dmg; FTPS only)
#   MANTA_FTP_KEYCHAIN_SERVICE  Keychain service (default manta-update-ftp)
#   MANTA_FTP_USER        FTP user (default: the account of the Keychain item)
#   MANTA_FTP_DATA_TLS    FTPS data channel: auto (default; clear data connections if
#                           the network resets encrypted ones), require, off
#   MANTA_UPDATE_KEY      private key (default ~/.config/manta/update-signing-key)
#   MANTA_UPDATE_STAGING  staging folder (default dist/update-staging)
#   MANTA_UPDATE_NEW_FEED 1 = start a new feed when the live one can't be fetched
#   MANTA_RELEASE_NOTES_DIR  release notes folder (default release-notes/)
#   MANTA_GITHUB_RELEASE  1 (default) = also create/update the GitHub release (gh, repo MANTA_GITHUB_REPO)
#   MANTA_SITE_PUBLISH    optional: path of an executable that publishes the website; run
#                           after a real stable publish (unset = skipped with a note)
#   MANTA_PUBLISH_SITE    1 (default) = run MANTA_SITE_PUBLISH after a real publish,
#                           0 = leave the website alone
#   plus everything scripts/build-release.sh understands (MANTA_SIGN_IDENTITY, MANTA_VERSION, …)
# The pre-rename RUSTLIFT_* names still work, with a deprecation warning.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

say()  { printf '\033[1m==> %s\033[0m\n' "$*"; }
warn() { printf '\033[33mwarning:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# Upload destinations from ~/.config/manta/publish.env (the environment wins).
. "$ROOT/scripts/lib/publish-env.sh"
load_publish_env

DEFAULT_FEED_URL=https://manta-filemanager.com/updates/update.json
DEFAULT_SEED_URL=
DEFAULT_DOWNLOAD_URL=https://manta-filemanager.com/download/
KC_SERVICE=${MANTA_FTP_KEYCHAIN_SERVICE:-manta-update-ftp}

# Deprecation aliases: accept the variable names from before the rename to Manta.
for name in UPDATE_URL UPDATE_DEST UPDATE_KEY UPDATE_STAGING UPDATE_NEW_FEED RELEASE_NOTES_DIR \
    FTP_USER UPDATE_PUBKEY VERSION DIST_DIR MIN_MACOS; do
    old="RUSTLIFT_$name" new="MANTA_$name"
    if [ -n "${!old+x}" ] && [ -z "${!new+x}" ]; then
        warn "$old is deprecated, use $new"
        export "$new=${!old}"
    fi
done

setup_help() {
    cat <<HELP
One-time setup for publishing Manta updates:

  1. Signing key (private key stays in ~/.config/manta/, public key goes into the app):
       cargo run -p manta-updater --bin manta-update-keygen
     then commit packaging/update-public-key.txt and back up ~/.config/manta/update-signing-key.

  2. FTP password into the macOS Keychain (prompts for the password; replace <ftp-user>):
       security add-generic-password -s $KC_SERVICE -a <ftp-user> -w
     (-U updates an existing item.) Check without revealing it:
       security find-generic-password -s $KC_SERVICE

  3. Upload destinations (not in the repository) in $PUBLISH_ENV_FILE (mode 600):
       MANTA_UPDATE_DEST=ftp://ftp.example.com/updates/
       MANTA_DOWNLOAD_DEST=ftp://ftp.example.com/download/
     (or user@host:/path/ for MANTA_UPDATE_DEST to upload over SSH). No passwords there.

  4. Release notes: release-notes/<version>.en.md and .de.md (the first run creates templates).

  5. Try it, then publish:
       scripts/publish-update.sh --dry-run
       scripts/publish-update.sh
     To forbid clear FTP data connections: MANTA_FTP_DATA_TLS=require scripts/publish-update.sh
HELP
}

DRY=0; SKIP_BUILD=0; CHANNEL=""; KEEP=10; DMG_TOO=1
while [ $# -gt 0 ]; do
    case "$1" in
        --dry-run) DRY=1 ;;
        --skip-build) SKIP_BUILD=1 ;;
        --no-dmg) DMG_TOO=0 ;;
        --channel) CHANNEL=${2:-}; shift ;;
        --keep) KEEP=${2:-}; shift ;;
        --setup-help) setup_help; exit 0 ;;
        -h|--help) awk 'NR > 1 { if (/^set -euo/) exit; print }' "$0"; exit 0 ;;
        *) die "unknown argument $1 (see --help)" ;;
    esac
    shift
done

FEED_URL=${MANTA_UPDATE_URL:-$DEFAULT_FEED_URL}
case "$FEED_URL" in
    https://*/update.json) ;;
    http://127.0.0.1[:/]*/update.json|http://localhost[:/]*/update.json) warn "local http feed (testing only)" ;;
    *) die "MANTA_UPDATE_URL must be https://…/update.json (got $FEED_URL)" ;;
esac
export MANTA_UPDATE_URL="$FEED_URL"

# Private scratch folder (mode 0700) for temporary files.
PRIV=$(umask 077; mktemp -d "${TMPDIR:-/tmp}/manta-publish.XXXXXX")
trap 'rm -rf "$PRIV"' EXIT

KEY=${MANTA_UPDATE_KEY:-$HOME/.config/manta/update-signing-key}
[ -f "$KEY" ] || { setup_help >&2; die "no signing key at $KEY (step 1 above)"; }
perm=$(stat -f %Lp "$KEY")
[ "$perm" = 600 ] || [ "$perm" = 400 ] || die "$KEY has mode $perm; run: chmod 600 \"$KEY\""

PUBKEY_FILE="$ROOT/packaging/update-public-key.txt"
if [ -n "${MANTA_UPDATE_PUBKEY:-}" ]; then
    PUBKEY_FILE="$PRIV/update-public-key.txt"
    printf '%s\n' "$MANTA_UPDATE_PUBKEY" > "$PUBKEY_FILE"
fi
[ -s "$PUBKEY_FILE" ] || die "no public key ($PUBKEY_FILE): the app would not accept updates. Run manta-update-keygen."

# From Cargo.toml (cargo pkgid reads Cargo.lock, which lags behind a bump).
VERSION=${MANTA_VERSION:-$(cargo metadata --no-deps --format-version 1 --offline 2>/dev/null \
    | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="manta"))')}
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || die "invalid version $VERSION"
if [ -z "$CHANNEL" ]; then
    case "$VERSION" in *-*) CHANNEL=beta ;; *) CHANNEL=stable ;; esac
fi
case "$CHANNEL" in stable|beta) ;; *) die "--channel must be stable or beta" ;; esac
[[ "$KEEP" =~ ^[1-9][0-9]*$ ]] || die "--keep must be a positive number"

DIST=${MANTA_DIST_DIR:-$ROOT/dist}
ZIP="$DIST/Manta-$VERSION.zip"
DMG="$DIST/Manta-$VERSION.dmg"
SEED_URL=${MANTA_UPDATE_SEED_URL-$DEFAULT_SEED_URL}
[ "$SEED_URL" != "$FEED_URL" ] || SEED_URL=""
DOWNLOAD_URL=${MANTA_DOWNLOAD_URL:-$DEFAULT_DOWNLOAD_URL}
DOWNLOAD_DEST=${MANTA_DOWNLOAD_DEST:-}
DEST=${MANTA_UPDATE_DEST:-}
# Checked before the (long) build: without destinations nothing could be uploaded.
if [ -z "$DEST" ] || { [ $DMG_TOO = 1 ] && [ -z "$DOWNLOAD_DEST" ]; }; then
    publish_env_help >&2
    [ -n "$DEST" ] || die "MANTA_UPDATE_DEST is not set (see above)"
    die "MANTA_DOWNLOAD_DEST is not set (see above), or pass --no-dmg"
fi
case "$DOWNLOAD_URL" in https://*/) ;; *) die "MANTA_DOWNLOAD_URL must be https://…/ (trailing slash)" ;; esac
STAGING=${MANTA_UPDATE_STAGING:-$DIST/update-staging}
say "Publishing Manta $VERSION ($CHANNEL) to $FEED_URL$([ $DRY = 1 ] && echo ' [dry run]')"

# ---------------------------------------------------------------- release notes
NOTES_DIR=${MANTA_RELEASE_NOTES_DIR:-$ROOT/release-notes}
NOTES_EN="$NOTES_DIR/$VERSION.en.md"
NOTES_DE="$NOTES_DIR/$VERSION.de.md"
MARK="TODO: replace this line"
if [ ! -f "$NOTES_EN" ] || [ ! -f "$NOTES_DE" ]; then
    mkdir -p "$NOTES_DIR"
    [ -f "$NOTES_EN" ] || printf '%s\n\n- New: …\n- Improved: …\n- Fixed: …\n' "$MARK (Markdown, shown in the update window)" > "$NOTES_EN"
    [ -f "$NOTES_DE" ] || printf '%s\n\n- Neu: …\n- Verbessert: …\n- Behoben: …\n' "$MARK (Markdown, wird im Update-Fenster gezeigt)" > "$NOTES_DE"
    die "release notes templates created:
    $NOTES_EN
    $NOTES_DE
  Write the notes (remove the TODO line), then run this script again."
fi
for f in "$NOTES_EN" "$NOTES_DE"; do
    ! grep -q "$MARK" "$f" || die "$f still contains the template line; write the notes first"
    [ -n "$(tr -d '[:space:]' < "$f")" ] || die "$f is empty"
done

# ---------------------------------------------------------------- build
if [ $SKIP_BUILD = 1 ]; then
    [ -f "$ZIP" ] || die "--skip-build: $ZIP does not exist"
    say "Using the existing $ZIP"
else
    MANTA_UPDATE_ZIP=1 "$ROOT/scripts/build-release.sh"
    [ -f "$ZIP" ] || die "the build did not produce $ZIP"
fi
if [ $DMG_TOO = 1 ] && [ ! -f "$DMG" ]; then
    die "$DMG does not exist (build it, or pass --no-dmg to publish the update only)"
fi

SIGN=(cargo run -q -p manta-updater --bin manta-update-sign --)

# ---------------------------------------------------------------- existing feed
# fetch URL OUT: sets FETCH_CODE to the HTTP status (000 = no answer). If HTTPS fails
# because of TLS/the certificate (e.g. before Let's Encrypt is enabled), sets TLS_BROKEN=1
# and asks over http:// instead, for presence only: whatever comes back is used only after
# its Ed25519 signature verifies with your key (manta-update-sign refuses anything else),
# and the app itself only ever talks HTTPS.
TLS_BROKEN=0
FETCH_CODE=000
# Optional HTTP Basic Auth for a password-protected site (pre-launch). Credentials come
# from the Keychain item $MANTA_SITE_AUTH_SERVICE (default manta-site-auth; account =
# user); the password goes to curl on stdin (-K -), never on the command line.
SITE_AUTH_SERVICE=${MANTA_SITE_AUTH_SERVICE:-manta-site-auth}
SITE_AUTH_USER=$(security find-generic-password -s "$SITE_AUTH_SERVICE" 2>/dev/null | sed -n 's/^ *"acct"<blob>="\(.*\)"$/\1/p' || true)
curl() {
    if [ -n "$SITE_AUTH_USER" ]; then
        { printf 'user = "%s:' "$SITE_AUTH_USER"; security find-generic-password -s "$SITE_AUTH_SERVICE" -w | tr -d '\n'; printf '"\n'; } \
            | command curl -K - "$@"
    else
        command curl "$@"
    fi
}
fetch() {
    local url=$1 out=$2 rc=0 h
    FETCH_CODE=$(curl -sS -L --max-time 60 -o "$out" -w '%{http_code}' -H 'Cache-Control: no-cache' "$url" 2>/dev/null) || rc=$?
    case "$rc" in
        0) ;;
        35|51|53|54|58|59|60|64|66|77|80|82|83|90|91)
            h=${url#*://}; h=${h%%/*}
            [ "$TLS_BROKEN" = 1 ] || warn "HTTPS to $h fails (curl error $rc: TLS/certificate); checking over http:// meanwhile. The app needs valid HTTPS: enable the certificate (Let's Encrypt)."
            TLS_BROKEN=1
            FETCH_CODE=$(curl -sS -L --proto-redir =http,https --max-time 60 -o "$out" -w '%{http_code}' \
                -H 'Cache-Control: no-cache' "http://${url#https://}" 2>/dev/null) || FETCH_CODE=000 ;;
        *) FETCH_CODE=000 ;;
    esac
    FETCH_CODE=${FETCH_CODE:-000}
}
rm -rf "$STAGING"
mkdir -p "$STAGING"
EXISTING=()
SEEDED=""
fetch "$FEED_URL" "$STAGING/.live.json"; code=$FETCH_CODE
case "$code" in
    200)
        fetch "$FEED_URL.sig" "$STAGING/.live.json.sig"
        [ "$FETCH_CODE" = 200 ] || die "the live feed has no update.json.sig; fix or remove the live update.json first"
        say "Merging into the live feed ($(grep -c '"version"' "$STAGING/.live.json" || true) release(s))"
        EXISTING=(--existing "$STAGING/.live.json" --existing-sig "$STAGING/.live.json.sig")
        ;;
    404|410)
        seed_ok=0
        if [ -n "$SEED_URL" ]; then
            fetch "$SEED_URL" "$STAGING/.live.json"
            if [ "$FETCH_CODE" = 200 ]; then
                fetch "$SEED_URL.sig" "$STAGING/.live.json.sig"
                [ "$FETCH_CODE" != 200 ] || seed_ok=1
            fi
            [ $seed_ok = 1 ] || warn "could not fetch the seed feed $SEED_URL (+ .sig); starting a new update.json"
        fi
        if [ $seed_ok = 1 ]; then
            say "No feed at $FEED_URL yet; starting from $SEED_URL ($(grep -c '"version"' "$STAGING/.live.json" || true) release(s))"
            EXISTING=(--existing "$STAGING/.live.json" --existing-sig "$STAGING/.live.json.sig")
            SEEDED=$SEED_URL
        else
            say "No live feed yet; starting a new update.json"
        fi
        ;;
    *)
        if [ $DRY = 1 ] || [ "${MANTA_UPDATE_NEW_FEED:-0}" = 1 ]; then
            warn "could not fetch $FEED_URL (HTTP $code); starting a new update.json"
        else
            die "could not fetch $FEED_URL (HTTP $code). Set MANTA_UPDATE_NEW_FEED=1 to start a new feed (drops older entries)"
        fi
        ;;
esac

# ---------------------------------------------------------------- sign
cp "$ZIP" "$STAGING/"
MIN_MACOS=${MANTA_MIN_MACOS:-11.0}
# The new entry gets an absolute URL next to the feed (older entries keep theirs; a
# relative URL is resolved against the feed URL the app asked for).
"${SIGN[@]}" manifest --key "$KEY" --zip "$STAGING/$(basename "$ZIP")" --version "$VERSION" \
    --url "${FEED_URL%/update.json}/$(basename "$ZIP")" --channel "$CHANNEL" --min-macos "$MIN_MACOS" --notes-en "$NOTES_EN" --notes-de "$NOTES_DE" \
    --keep "$KEEP" --out "$STAGING" ${EXISTING[@]+"${EXISTING[@]}"}
rm -f "$STAGING/.live.json" "$STAGING/.live.json.sig"
# Check with the public key the app embeds (catches a key mismatch before users do).
"${SIGN[@]}" verify --pubkey "$PUBKEY_FILE" --manifest "$STAGING/update.json" --zip "$STAGING/$(basename "$ZIP")"

# A seeded feed's entries with relative URLs ("Manta-0.1.3.zip") now resolve next to the
# new feed, so their ZIPs are mirrored from the seed feed's folder (checked against the
# signed size and SHA-256) and uploaded with this release.
MIRRORED=()
if [ -n "$SEEDED" ]; then
    while IFS=$'\t' read -r mver mname msize msha; do
        [ -n "$mname" ] || continue
        say "Mirroring $mname ($mver) from ${SEEDED%/update.json}/"
        fetch "${SEEDED%/update.json}/$mname" "$STAGING/$mname"
        [ "$FETCH_CODE" = 200 ] || die "could not download ${SEEDED%/update.json}/$mname"
        [ "$(stat -f %z "$STAGING/$mname")" = "$msize" ] || die "$mname: size differs from the signed entry"
        [ "$(shasum -a 256 "$STAGING/$mname" | cut -d' ' -f1)" = "$msha" ] || die "$mname: SHA-256 differs from the signed entry"
        MIRRORED+=("$STAGING/$mname")
    done < <(python3 -I - "$STAGING/update.json" "$VERSION" <<'PY'
import json, sys
m = json.load(open(sys.argv[1]))
for r in m["releases"]:
    u = r["url"]
    if r["version"] != sys.argv[2] and "://" not in u and not u.startswith("/") and "/" not in u:
        print("\t".join([r["version"], u, str(r["size"]), r["sha256"].lower()]))
PY
)
fi

# Download folder: the DMG, for stable releases also Manta.dmg + latest.json.
DL_FILES=()
if [ $DMG_TOO = 1 ]; then
    DLSTAGE="$STAGING/download"
    mkdir -p "$DLSTAGE"
    DMGNAME=$(basename "$DMG")
    cp -c "$DMG" "$DLSTAGE/$DMGNAME" 2>/dev/null || cp "$DMG" "$DLSTAGE/$DMGNAME"
    DL_FILES=("$DLSTAGE/$DMGNAME")
    if [ "$CHANNEL" = stable ]; then
        cp -c "$DMG" "$DLSTAGE/Manta.dmg" 2>/dev/null || cp "$DMG" "$DLSTAGE/Manta.dmg"
        python3 -I - "$DLSTAGE/latest.json" "$VERSION" "$DMGNAME" "$DOWNLOAD_URL" \
            "$(stat -f %z "$DMG")" "$(shasum -a 256 "$DMG" | cut -d' ' -f1)" <<'PY'
import json, sys, datetime
out, version, dmg, base, size, sha = sys.argv[1:]
doc = {
    "version": version,
    "dmg": dmg,
    "url": base + dmg,
    "latest_url": base + "Manta.dmg",
    "size": int(size),
    "sha256": sha,
    "published": datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
}
with open(out, "w") as f:
    json.dump(doc, f, indent=2)
    f.write("\n")
PY
        DL_FILES+=("$DLSTAGE/Manta.dmg" "$DLSTAGE/latest.json")
    else
        say "$CHANNEL release: only $DMGNAME goes to the download folder (Manta.dmg and latest.json stay)"
    fi
fi

# ---------------------------------------------------------------- upload
ZIPNAME=$(basename "$ZIP")
if [ ${#DL_FILES[@]} -gt 0 ]; then
    case "$DOWNLOAD_DEST" in ftp://*/) ;; *) die "MANTA_DOWNLOAD_DEST must look like ftp://host/path/ (trailing slash)" ;; esac
fi
case "$DEST" in
    ftp://*/) MODE=ftp ;;
    ftps://*|ftp://*) die "MANTA_UPDATE_DEST must look like ftp://host/path/ (explicit TLS is always used; trailing slash)" ;;
    *:*/) MODE=ssh ;;
    *) die "MANTA_UPDATE_DEST must look like ftp://host/path/ or user@host:/path/ (trailing slash)" ;;
esac

if [ "$MODE" = ftp ] || [ ${#DL_FILES[@]} -gt 0 ]; then
    if [ "$MODE" = ftp ]; then host=${DEST#ftp://}; else host=${DOWNLOAD_DEST#ftp://}; fi
    host=${host%%/*}
    case "${MANTA_FTP_DATA_TLS:-auto}" in auto|require|off) ;; *) die "MANTA_FTP_DATA_TLS must be auto, require or off" ;; esac
    # The uploader (Manta's own FTPS backend), built once in release mode.
    say "Building manta-update-upload"
    cargo build --release -q -p manta-update-upload || die "could not build manta-update-upload"
    UPLOAD="${CARGO_TARGET_DIR:-$ROOT/target}/release/manta-update-upload"
    [ -x "$UPLOAD" ] || die "manta-update-upload not found at $UPLOAD"

    # Account name from the Keychain item (attributes only; no password is read here).
    FTP_USER=${MANTA_FTP_USER:-}
    if [ -z "$FTP_USER" ]; then
        FTP_USER=$(security find-generic-password -s "$KC_SERVICE" 2>/dev/null \
            | sed -n 's/^ *"acct"<blob>="\(.*\)"$/\1/p' | head -n 1) || true
    fi
fi

if [ $DRY = 1 ]; then
    say "Dry run: staged in $STAGING (nothing uploaded)"
    ls -l "$STAGING" ${DL_FILES[@]+"$STAGING/download"}
    [ ! -f "$STAGING/download/latest.json" ] || cat "$STAGING/download/latest.json"
    echo
    if [ "$MODE" = ftp ]; then
        if [ -n "$FTP_USER" ]; then
            say "Upload setup: FTPS to $DEST as the Keychain account of \"$KC_SERVICE\" (found)"
        else
            warn "no Keychain item \"$KC_SERVICE\" yet; create it with:
    security add-generic-password -s $KC_SERVICE -a <ftp-user> -w"
        fi
        echo "A real run executes (password on stdin, never on the command line):"
        printf '    <password> | %q --user %q --data-channel %q %q' "$UPLOAD" "${FTP_USER:-<ftp-user>}" \
            "${MANTA_FTP_DATA_TLS:-auto}" "$DEST"
        printf ' %q' ${MIRRORED[@]+"${MIRRORED[@]}"} "$STAGING/$ZIPNAME" "$STAGING/update.json.sig" "$STAGING/update.json"
        echo
    else
        echo "A real run uploads with rsync/scp over SSH to $DEST"
    fi
    if [ ${#DL_FILES[@]} -gt 0 ]; then
        printf '    <password> | %q --user %q --data-channel %q %q' "$UPLOAD" "${FTP_USER:-<ftp-user>}" \
            "${MANTA_FTP_DATA_TLS:-auto}" "$DOWNLOAD_DEST"
        printf ' %q' "${DL_FILES[@]}"
        echo
    fi
    echo
    echo "Publish for real:  scripts/publish-update.sh --skip-build"
    exit 0
fi

if [ "$MODE" = ftp ] || [ ${#DL_FILES[@]} -gt 0 ]; then
    [ -n "$FTP_USER" ] || { setup_help >&2; die "no FTP account: create the Keychain item \"$KC_SERVICE\" (step 2 above) or set MANTA_FTP_USER"; }
    # The password goes from `security` straight into a variable and from there
    # (printf is a shell builtin, so it never shows up in ps) to the uploader's stdin.
    pw=$(security find-generic-password -s "$KC_SERVICE" -a "$FTP_USER" -w 2>/dev/null) || pw=""
    if [ -z "$pw" ]; then
        [ -t 0 ] || die "no password in the Keychain item \"$KC_SERVICE\" for $FTP_USER"
        read -r -s -p "FTP password for $FTP_USER@$host (not stored): " pw; echo
    fi
fi
if [ "$MODE" = ftp ]; then
    # ZIP(s) first, then the signature, then the feed: the feed never points to a
    # ZIP that isn't there yet.
    say "Uploading ${#MIRRORED[@]} mirrored ZIP(s), $ZIPNAME, update.json.sig and update.json (FTPS to $DEST)"
    printf '%s\n' "$pw" | "$UPLOAD" --user "$FTP_USER" --data-channel "${MANTA_FTP_DATA_TLS:-auto}" "$DEST" \
        ${MIRRORED[@]+"${MIRRORED[@]}"} "$STAGING/$ZIPNAME" "$STAGING/update.json.sig" "$STAGING/update.json" \
        || { unset pw; die "FTPS upload failed (see above)"; }
else
    if command -v rsync >/dev/null; then
        say "Uploading $ZIPNAME (rsync over ssh)"
        rsync -t --chmod=F644 --partial -e ssh "$STAGING/$ZIPNAME" "$DEST"
        say "Uploading update.json and update.json.sig"
        # --delay-updates: both files appear together at the end of the transfer.
        rsync -t --chmod=F644 --delay-updates -e ssh "$STAGING/update.json" "$STAGING/update.json.sig" "$DEST"
    else
        say "Uploading with scp"
        scp -q "$STAGING/$ZIPNAME" "$DEST"
        scp -q "$STAGING/update.json.sig" "$STAGING/update.json" "$DEST"
    fi
    [ ${#MIRRORED[@]} = 0 ] || warn "the mirrored ZIPs were not uploaded over SSH; copy them by hand: ${MIRRORED[*]}"
fi
if [ ${#DL_FILES[@]} -gt 0 ]; then
    # Versioned DMG, then Manta.dmg, then latest.json (the website shows what is there).
    say "Uploading the DMG$([ ${#DL_FILES[@]} -gt 1 ] && echo ', Manta.dmg and latest.json') (FTPS to $DOWNLOAD_DEST)"
    printf '%s\n' "$pw" | "$UPLOAD" --user "$FTP_USER" --data-channel "${MANTA_FTP_DATA_TLS:-auto}" "$DOWNLOAD_DEST" \
        "${DL_FILES[@]}" || { unset pw; die "FTPS upload of the download files failed (the update feed is already live)"; }
fi
unset pw

# ---------------------------------------------------------------- check the live feed
say "Checking the live feed"
LIVE="$PRIV/live"; mkdir -p "$LIVE"
fetch "$FEED_URL" "$LIVE/update.json"
[ "$FETCH_CODE" = 200 ] || die "$FEED_URL is not reachable after the upload (HTTP $FETCH_CODE)"
fetch "$FEED_URL.sig" "$LIVE/update.json.sig"
[ "$FETCH_CODE" = 200 ] || die "$FEED_URL.sig is not reachable after the upload"
"${SIGN[@]}" verify --pubkey "$PUBKEY_FILE" --manifest "$LIVE/update.json"
cmp -s "$LIVE/update.json" "$STAGING/update.json" \
    || warn "the live update.json differs from the uploaded one (a cache in between?); see the .htaccess in docs/updates.md"
head_code() {
    local url=$1 code rc=0
    code=$(curl -sS -o /dev/null -I -w '%{http_code}' --max-time 30 "$url" 2>/dev/null) || rc=$?
    if [ "$rc" != 0 ] && [ "$TLS_BROKEN" = 1 ]; then
        code=$(curl -sS -o /dev/null -I -w '%{http_code}' --max-time 30 "http://${url#https://}" 2>/dev/null) || code=000
        code="$code (over http)"
    fi
    echo "${code:-000}"
}
for f in "$ZIPNAME" ${MIRRORED[@]+"${MIRRORED[@]##*/}"}; do
    zcode=$(head_code "${FEED_URL%/update.json}/$f")
    [ "${zcode%% *}" = 200 ] || warn "${FEED_URL%/update.json}/$f answered HTTP $zcode"
done
for f in ${DL_FILES[@]+"${DL_FILES[@]##*/}"}; do
    dcode=$(head_code "$DOWNLOAD_URL$f")
    [ "${dcode%% *}" = 200 ] || warn "$DOWNLOAD_URL$f answered HTTP $dcode"
done
[ "$TLS_BROKEN" = 0 ] || warn "HTTPS for ${FEED_URL#https://} does not verify yet: the app cannot update until the certificate is enabled"
say "Published Manta $VERSION ($CHANNEL): $FEED_URL"
[ ${#DL_FILES[@]} = 0 ] || say "Download: $DOWNLOAD_URL${DL_FILES[0]##*/}$([ ${#DL_FILES[@]} -gt 1 ] && echo " (and ${DOWNLOAD_URL}Manta.dmg)")"

# ---------------------------------------------------------------- website
# The website is maintained outside this repository. If MANTA_SITE_PUBLISH names its
# publisher (e.g. one that regenerates the changelog from release-notes/), it runs after
# every stable release. The update is already live: a failure here only warns.
SITE_PUBLISH=${MANTA_SITE_PUBLISH:-}
case "${MANTA_PUBLISH_SITE:-1}" in
    1) if [ "$CHANNEL" != stable ]; then
           say "Website not published for a $CHANNEL release"
       elif [ -z "$SITE_PUBLISH" ]; then
           say "Website not published: no site publisher configured (MANTA_SITE_PUBLISH)"
       elif [ ! -x "$SITE_PUBLISH" ]; then
           warn "MANTA_SITE_PUBLISH=$SITE_PUBLISH is not an executable file; website not published"
       else
           say "Publishing the website ($SITE_PUBLISH; MANTA_PUBLISH_SITE=0 skips this)"
           MANTA_REPO=${MANTA_REPO:-$ROOT} "$SITE_PUBLISH" \
               || warn "publishing the website failed (the update is live); run $SITE_PUBLISH again"
       fi
       ;;
    0) say "Website not published (MANTA_PUBLISH_SITE=0)" ;;
    *) warn "MANTA_PUBLISH_SITE must be 0 or 1 (got \"$MANTA_PUBLISH_SITE\"); website not published" ;;
esac

# ---------------------------------------------------------------- GitHub release
# The same release, with the DMG and the notes, on GitHub (MANTA_GITHUB_RELEASE=0 skips it).
# Needs `gh` logged in with write access to $MANTA_GITHUB_REPO and the tag pushed or
# created from the current HEAD. The update feed stays on the website.
GH_REPO=${MANTA_GITHUB_REPO:-web-workers-com/manta}
if [ "${MANTA_GITHUB_RELEASE:-1}" = 1 ] && [ $DRY = 0 ]; then
    if ! command -v gh >/dev/null 2>&1; then
        warn "gh not installed: no GitHub release created"
    else
        tag="v$VERSION"
        notes=$(mktemp "${TMPDIR:-/tmp}/manta-notes.XXXXXX")
        { cat "$NOTES_DIR/$VERSION.en.md"; printf '\n---\n\n<details><summary>Deutsch</summary>\n\n'; cat "$NOTES_DIR/$VERSION.de.md"; printf '\n</details>\n\nAuto-updates come from https://manta-filemanager.com (signed with Ed25519). macOS 11+ on Apple silicon.\n'; } > "$notes"
        dmg="$DIST/Manta-$VERSION.dmg"
        pre=""; [ "$CHANNEL" = stable ] || pre="--prerelease"
        if gh release view "$tag" -R "$GH_REPO" >/dev/null 2>&1; then
            say "GitHub release $tag exists: updating notes and DMG"
            gh release edit "$tag" -R "$GH_REPO" --notes-file "$notes" >/dev/null || warn "could not update the GitHub release notes"
            [ -f "$dmg" ] && { gh release upload "$tag" "$dmg" -R "$GH_REPO" --clobber || warn "could not upload the DMG to GitHub"; }
        else
            say "Creating GitHub release $tag on $GH_REPO"
            gh release create "$tag" ${dmg:+"$dmg"} -R "$GH_REPO" --title "Manta $VERSION" --notes-file "$notes" \
                --target "$(git -C "$ROOT" rev-parse HEAD)" $pre \
                || warn "creating the GitHub release failed (push the commit first?); the website release is live"
        fi
        rm -f "$notes"
    fi
fi
