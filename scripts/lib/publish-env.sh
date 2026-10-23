# Sourced by scripts/publish-update.sh.
#
# Loads the local, untracked publishing configuration: where the update feed and the
# downloads are uploaded to (and an optional MANTA_SITE_PUBLISH). The repository
# deliberately contains no server names; each maintainer keeps them in
#
#   ~/.config/manta/publish.env        (or the file named by MANTA_PUBLISH_ENV)
#
# Format: one MANTA_NAME=value per line, '#' starts a comment, optional `export ` and
# quotes. Variables already set in the environment win over the file. Example:
#
#   MANTA_UPDATE_DEST=ftp://ftp.example.com/updates/
#   MANTA_DOWNLOAD_DEST=ftp://ftp.example.com/download/
#   # MANTA_FTP_USER=ftp-user     (optional; default: the account of the Keychain item)
#
# Never put a password in this file: the FTP password stays in the macOS Keychain.

PUBLISH_ENV_FILE=${MANTA_PUBLISH_ENV:-$HOME/.config/manta/publish.env}

load_publish_env() {
    local f=$PUBLISH_ENV_FILE line key val
    [ -f "$f" ] || return 0
    while IFS= read -r line || [ -n "$line" ]; do
        line=${line%$'\r'}
        line=${line#"${line%%[![:space:]]*}"}
        case "$line" in ''|'#'*) continue ;; esac
        line=${line#export }
        key=${line%%=*}
        val=${line#*=}
        if [ "$key" = "$line" ] || ! [[ "$key" =~ ^MANTA_[A-Z0-9_]+$ ]]; then
            printf '\033[33mwarning:\033[0m %s: ignoring line "%s" (expected MANTA_NAME=value)\n' "$f" "$line" >&2
            continue
        fi
        case "$key" in
            *PASS*|*SECRET*|*TOKEN*)
                printf '\033[33mwarning:\033[0m %s: ignoring %s (passwords belong in the Keychain)\n' "$f" "$key" >&2
                continue ;;
        esac
        case "$val" in
            \"*\") val=${val#\"}; val=${val%\"} ;;
            \'*\') val=${val#\'}; val=${val%\'} ;;
        esac
        [ -n "${!key+x}" ] || export "$key=$val"
    done < "$f"
}

# Prints how to create the config file (for error messages).
publish_env_help() {
    cat <<HELP
Upload destinations are not part of the repository. Create $PUBLISH_ENV_FILE
(mode 600), for example:

  mkdir -p "$(dirname "$PUBLISH_ENV_FILE")"
  cat > "$PUBLISH_ENV_FILE" <<'EOF'
MANTA_UPDATE_DEST=ftp://ftp.example.com/updates/
MANTA_DOWNLOAD_DEST=ftp://ftp.example.com/download/
EOF
  chmod 600 "$PUBLISH_ENV_FILE"

or set these variables in the environment. The FTP password stays in the Keychain.
HELP
}
