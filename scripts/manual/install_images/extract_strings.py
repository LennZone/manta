#!/usr/bin/env python3
"""Read Apple's own localized strings for the install illustrations from this Mac
and write them to strings.json (next to this script):

    python3 scripts/manual/install_images/extract_strings.py          # write strings.json
    python3 scripts/manual/install_images/extract_strings.py --check  # exit 1 if it would change

The Gatekeeper dialogs, the Privacy & Security pane and the System Settings sidebar are
macOS UI, so their text must be exactly what macOS shows in each language. The strings
come from the system's .loctable files (plutil reads them); every entry in strings.json
records the table and key it came from. generate.py only reads strings.json, so the
images can be rebuilt on any Mac, and this script re-checks the strings after a macOS
update. Python 3 standard library only.
"""
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "strings.json"

# manual language -> Apple localization
LANGS = {"de": "de", "en": "en", "zh-Hans": "zh_CN", "es": "es", "fr": "fr", "it": "it", "ru": "ru"}

UIAGENT = "/System/Library/CoreServices/CoreServicesUIAgent.app/Contents/Resources"
EXT = "/System/Library/ExtensionKit/Extensions"
SETTINGS = "/System/Applications/System Settings.app/Contents/Resources"

# name -> (loctable, key[, default when a language has no entry])
SOURCES = {
    # first launch: "“Manta” Not Opened"
    "blocked_title": (f"{UIAGENT}/QuarantineHeadlines.loctable", "Q_HEADLINE_SUNFISH_NOT_VERIFIED"),
    "blocked_text": (f"{UIAGENT}/Quarantine.loctable", "Q_DETAIL_CASPIAN_UNVERIFIED"),
    "done": (f"{UIAGENT}/Quarantine.loctable", "Q_BUTTON_DONE"),
    "move_to_trash": (f"{UIAGENT}/Quarantine.loctable", "Q_BUTTON_MOVE_TO_TRASH"),
    # after Open Anyway in System Settings: "Open “Manta”?"
    "confirm_title": (f"{UIAGENT}/QuarantineHeadlines.loctable", "Q_HEADLINE_SUNFISH_NOT_VERIFIED_ALLOW"),
    "confirm_text": (f"{UIAGENT}/Quarantine.loctable", "Q_DETAIL_CASPIAN_ALLOW_UNVERIFIED"),
    "open_anyway_dialog": (f"{UIAGENT}/CodeEvaluation.loctable", "OPEN_ANYWAY_BUTTON"),
    "done_dialog": (f"{UIAGENT}/CodeEvaluation.loctable", "DONE_BUTTON"),
    # System Settings › Privacy & Security
    "settings_app": (f"{SETTINGS}/InfoPlist.loctable", "CFBundleName"),
    "search": (f"{SETTINGS}/Localizable.loctable", "SEARCH_LABEL"),
    "privacy_security": (f"{EXT}/SecurityPrivacyExtension.appex/Contents/Resources/InfoPlist.loctable", "CFBundleDisplayName"),
    "security": (f"{EXT}/SecurityPrivacyExtension.appex/Contents/Resources/Localizable.loctable", "Security"),
    "allow_apps_from": (f"{EXT}/SecurityPrivacyExtension.appex/Contents/Resources/Localizable.loctable", "Allow applications from"),
    "app_store_known": (f"{EXT}/SecurityPrivacyExtension.appex/Contents/Resources/Localizable.loctable", "App Store & Known Developers"),
    "was_blocked": (f"{EXT}/SecurityPrivacyExtension.appex/Contents/Resources/Localizable.loctable", "“%@” was blocked to protect your Mac."),
    "open_anyway": (f"{EXT}/SecurityPrivacyExtension.appex/Contents/Resources/Localizable.loctable", "Open Anyway"),
    "analytics": (f"{EXT}/SecurityPrivacyExtension.appex/Contents/Resources/Localizable.loctable", "ANALYTICS"),
    "advertising": (f"{EXT}/SecurityPrivacyExtension.appex/Contents/Resources/Localizable.loctable", "ADVERTISING"),
    "filevault": (f"{EXT}/SecurityPrivacyExtension.appex/Contents/Resources/Localizable.loctable", "FileVault"),
    # sidebar (order from System Settings' Sidebar.plist)
    "sb_general": (f"{SETTINGS}/Localizable.loctable", "GENERAL"),
    "sb_appearance": (f"{EXT}/Appearance.appex/Contents/Resources/InfoPlist.loctable", "CFBundleDisplayName"),
    "sb_accessibility": (f"{EXT}/AccessibilitySettingsExtension.appex/Contents/Resources/InfoPlist.loctable", "CFBundleDisplayName"),
    "sb_menu_bar": (f"{EXT}/ControlCenterSettings.appex/Contents/Resources/InfoPlist.loctable", "CFBundleDisplayName"),
    "sb_siri": (f"{EXT}/SiriPreferenceExtension.appex/Contents/Resources/InfoPlist.loctable", "CFBundleDisplayName"),
    "sb_spotlight": (f"{EXT}/SpotlightPreferenceExtension.appex/Contents/Resources/InfoPlist.loctable", "CFBundleDisplayName"),
    "sb_desktop_dock": (f"{EXT}/DesktopSettings.appex/Contents/Resources/InfoPlist.loctable", "CFBundleDisplayName"),
    "sb_displays": (f"{EXT}/DisplaysExt.appex/Contents/Resources/InfoPlist.loctable", "CFBundleDisplayName"),
    "sb_wallpaper": (f"{EXT}/Wallpaper.appex/Contents/Resources/InfoPlist.loctable", "CFBundleDisplayName", "Wallpaper"),
}


def load(path, cache={}):
    if path not in cache:
        r = subprocess.run(["plutil", "-convert", "json", "-o", "-", path], capture_output=True)
        if r.returncode != 0:
            sys.exit(f"extract_strings.py: cannot read {path}: {r.stderr.decode().strip()}")
        cache[path] = json.loads(r.stdout)
    return cache[path]


def clean(s):
    # soft hyphens are line-break hints in Apple's tables, not visible text
    return s.replace("­", "")


def main():
    ver = subprocess.run(["sw_vers", "-productVersion"], capture_output=True, text=True).stdout.strip()
    out = {"_comment": "Generated by extract_strings.py from this Mac's macOS localizations; "
                       "do not edit by hand.", "_macos": ver, "strings": {}}
    for name, src in SOURCES.items():
        path, key = src[0], src[1]
        table = load(path)
        entry = {"source": f"{path.rsplit('/Contents/Resources/', 1)[0].rsplit('/', 1)[-1]}"
                           f" › {path.rsplit('/', 1)[-1]} › {key}"}
        for lang, apple in LANGS.items():
            v = table.get(apple, {}).get(key)
            if v is None and len(src) > 2:
                v = src[2]
            if v is None:
                sys.exit(f"extract_strings.py: {key} missing for {apple} in {path}")
            entry[lang] = clean(v)
        out["strings"][name] = entry
    text = json.dumps(out, ensure_ascii=False, indent=2) + "\n"
    old = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
    # the macOS version line alone is no change
    strip = lambda t: "\n".join(l for l in t.splitlines() if '"_macos"' not in l)
    if "--check" in sys.argv[1:]:
        if strip(old) != strip(text):
            print("extract_strings.py: strings.json is out of date", file=sys.stderr)
            sys.exit(1)
        print("extract_strings.py: strings.json is up to date")
        return
    if strip(old) != strip(text):
        OUT.write_text(text, encoding="utf-8")
        print(f"extract_strings.py: wrote {OUT.name} (macOS {ver})")
    else:
        print("extract_strings.py: no change")


if __name__ == "__main__":
    main()
