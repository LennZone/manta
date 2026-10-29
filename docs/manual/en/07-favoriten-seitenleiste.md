# Favorites & Sidebar {#seitenleiste}

The sidebar on the left takes you to your most important places in one click: favorites, standard folders, drives, connected servers, containers and cloud storage. A click opens the place in the active pane.

![Sidebar with all sections](images/sidebar.png)

## The sections {#abschnitte}

| Section | Contents |
|---|---|
| **Favorites** | Folders and server connections you've kept. |
| **Locations** | Home, Applications, Desktop, Documents, Downloads and, at the bottom, the **Trash** (see below). |
| **Cloud** | iCloud Drive and the folders of the cloud apps (Google Drive for desktop, OneDrive, Dropbox, Box, …), see [Cloud Storage](09-cloud.md). The section only appears when Manta finds at least one cloud storage. |
| **Devices** | Macintosh HD and every mounted volume (USB drives, disks, network shares, disk images). |
| **Servers** | Servers you're connected to. Without a connection it shows **Connect to Server…**. |
| **Containers** | Running and stopped Docker containers and Kubernetes pods (only if Docker is installed or a Kubernetes context is set). |

The circular-arrow button next to **Cloud** and **Devices** looks for cloud folders and volumes again. Manta also notices new cloud apps by itself.

Hover over a place to see its full path in a tooltip.

## Add a folder to Favorites {#favorit-hinzufuegen}

There are three ways:

- **Keyboard**: open the folder in the active pane and press **⌃⌘T** (**Add Folder to Favorites**).
- **Mouse**: click the **+** next to the **Favorites** heading ("Add Current Folder"), or choose **Add Folder to Favorites** in the context menu of an empty spot in the pane.
- **Drag**: drag one or more folders from a pane onto the **Favorites** section (see below).

Folders **inside archives** can't be favorites ("Folders inside archives can't be added to Favorites").

### Drag folders onto Favorites {#favoriten-ziehen}

1. Select one or more folders in a pane – in any view, local or on a server.
2. Drag them onto the **Favorites** section. The section is highlighted, a line shows where the folders will go, and the pointer says "Add 2 folders to Favorites", for example.
3. Let go. Manta reports "Added “Projects” to Favorites" or "Added 2 folders to Favorites".

Good to know:

- **Folders only**: if you drag only files, the pointer says "Only folders can be favorites" and nothing happens. With a mixed selection Manta takes just the folders.
- **No duplicates**: if a folder is already a favorite, the existing entry flashes briefly and Manta says "“name” is already in Favorites". Only the place counts, not the favorite's name.
- **Server folders**: dragging a folder from a server creates a server favorite with that folder as its start folder, such as "files.example.com – www". No new password is stored with it; if the Keychain already has one for this server account (from another favorite), the new favorite uses it too. Otherwise Manta asks when you open it (unless you sign in with the ssh-agent or a key file without a passphrase).

![A folder being dragged onto Favorites](images/favorites-drag.png)

## Reorder and remove favorites {#favoriten-ordnen}

- **Reorder**: drag a favorite up or down in the list. The dragged row fades and a line shows the new position. The order is saved right away.
- **Remove**: right-click the favorite → **Remove from Favorites**, or drag the favorite onto the **Trash** in the sidebar. The folder itself stays, of course.

## Trash {#papierkorb}

The **Trash** is the last row under **Locations**, as in Finder. Its icon shows whether it is empty or full (a filled bin = something is in it); the tooltip gives the number of items.

- **Open**: click **Trash**. It opens in the active pane like any folder. **Delete Immediately…** (or **⌘⌫**) deletes single items in it for good – Manta asks first.
- **Empty**: right-click **Trash** → **Empty Trash…**, or press **⇧⌘⌫**, or choose **Empty Trash…** in the command palette. Manta asks (“Are you sure you want to permanently erase the items in the Trash?”) and shows the number of items and their size. You can't undo this. Items that can't be deleted (locked ones, for example) are skipped and reported.
- **Drag onto it**: drag items from a pane onto **Trash**. The row is highlighted and the pointer says e.g. “Move 3 items to the Trash”. Local items go to the Trash without asking (like **⌘⌫**). Items on a **server**, in **S3** or in a **container** can't go to a Trash: Manta asks whether to delete them permanently. Items in an **archive** can't be deleted – the pointer shows it. You can also drag files from other apps (Finder, for example) onto the Trash.
- **Put Back**: only in Finder (right-click → **Open in Finder**, then **Put Back**). Finder keeps the original location in its own data, which Manta can't read.

### Full Disk Access {#papierkorb-zugriff}

macOS protects the Trash: to show what is in it, Manta needs **Full Disk Access**. Without it the pane says “Manta needs Full Disk Access to show the Trash” and offers two buttons:

1. **Open Full Disk Access** opens **System Settings › Privacy & Security › Full Disk Access**. Turn on **Manta** there and open the Trash again (**⌘R**). Sometimes Manta has to be restarted for this.
2. **Open in Finder** shows the Trash in Finder – even when Manta is your default file manager.

Without Full Disk Access, dragging onto the Trash and **⌘⌫** still work, and the row still shows whether the Trash is empty or full. **Empty Trash…** is then done by Finder (macOS asks once whether Manta may control Finder); Finder also empties the Trash of other volumes.

## Servers as favorites {#server-favoriten}

A server connection becomes a favorite when you turn on **Add to Favorites** in the **Connect to Server** dialog. You can give it a name of its own ("Name in the sidebar (optional)") and a start folder.

Right-click a server favorite:

- **Open** – connects and opens the start folder.
- **Edit Connection…** – opens the connection dialog with the saved details.
- **Remove from Favorites**.

Pressing **⌃⌘T** in a folder on a server creates a server favorite with that folder as its start folder. The password isn't copied.

See [Connections](08-verbindungen.md#favoriten).

## Drives {#geraete}

Newly connected drives and opened disk images (for example a DMG you install an app from) appear in the sidebar by themselves: internal volumes and network shares under **Devices**, ejectable drives under **Ejectable** (see below). If one doesn't show up, click the arrow icon next to **Devices** (**Rescan Volumes**).

As in Finder, disk images, USB sticks and external drives are listed in their own section, **Ejectable**, with the volume's own icon (or that of the app on it) if it has one. The eject icon next to it ejects the drive. The context menu also offers **Open**, **Open in New Tab**, **Show in Enclosing Folder** (for a disk image, the DMG file) and **Eject “Name”**. Panes that show the drive switch to **Home** when it is ejected. If an app is still using the drive, Manta reports that it couldn't be ejected.

## Connected servers {#server}

Every open connection is listed under **Servers** with a green dot. A click opens it in the active pane. The eject icon that appears on hover closes the connection (**Disconnect**).

## Containers {#container}

The **Containers** section lists your Docker containers, grouped by Compose project as in Docker Desktop. Running containers have a coloured dot; stopped ones are dimmed and collapsed under "Stopped (N)". For the current Kubernetes context there is a namespace picker with pods and their containers.

**Start, restart, stop**: right-click a Docker container. A stopped container offers **Start**, a running one **Restart** and **Stop**. Manta runs `docker start`, `docker restart` or `docker stop`; meanwhile the row says “Starting…” (etc.), and a message confirms the result. If a container stops while it is open in a pane, the pane offers **Start Container** instead of **Try Again**, and lists the folder again once it runs.

The list refreshes in the background every 5 seconds. The arrow icon (**Refresh Containers**) reads it again right away. See [Connections](08-verbindungen.md#container).

## Where favorites are stored {#speicherort}

In `~/Library/Application Support/Manta/favorites.json`. Passwords aren't in it: with **Remember password** they go to the macOS Keychain, see [Connections](08-verbindungen.md#favoriten).
