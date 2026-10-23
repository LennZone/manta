# dmgbuild settings for the Manta installer window: the app on the left,
# an Applications shortcut on the right, an arrow background between them.
# Used by scripts/build-release.sh as `dmgbuild -s <this> -D app=... -D bg=...`.
# dmgbuild writes the Finder layout (.DS_Store) itself, so no Finder
# automation permission is needed.

import os.path

app = defines["app"]  # noqa: F821 (provided by dmgbuild)
background = defines["bg"]  # noqa: F821; background.png, background@2x.png next to it
volume_icon = defines.get("volicon")  # noqa: F821

format = "UDZO"
compression_level = 9
filesystem = "HFS+"

files = [app]
symlinks = {"Applications": "/Applications"}
if volume_icon and os.path.exists(volume_icon):
    icon = volume_icon

# Window: 660x400 content, matching the background image.
window_rect = ((200, 120), (660, 400))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
arrange_by = None
icon_size = 128
text_size = 13
icon_locations = {
    os.path.basename(app): (165, 190),
    "Applications": (495, 190),
}
