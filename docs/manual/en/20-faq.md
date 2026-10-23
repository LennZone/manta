# FAQ {#faq}

Short answers to frequently asked questions. The linked chapters have the details.

## General

### Does Manta replace Finder? {#finder-ersetzen}

Not entirely. You can make Manta your default file manager; then "Show in Finder" and (up to macOS 15) folders from other apps open in Manta. But the Desktop, the Finder icon in the Dock and double-clicks inside Finder windows always stay with Finder – macOS doesn't allow any app to take them. See [Default file manager](01-erste-schritte.md#standard-dateimanager).

### Does Manta cost anything or collect data? {#daten}

Manta collects no usage data. It only goes online for your own connections and the update check, which sends only your IP address and the Manta version. See [Privacy](17-updates.md#datenschutz).

Manta is free and open source under the MIT licence: you may use, change and share it freely. The source code is on [GitHub](https://github.com/web-workers-com/manta) – in the app under **Help** → **Source Code on GitHub…** – and contributions are welcome. News, help and contact are on the website [manta-filemanager.com](https://manta-filemanager.com/) – in the app under **Help** → **Manta Website & Support…** and in Settings under [About Manta](16-einstellungen.md#ueber).

Manta is built on open-source components. Their licence notices are under **Help** → **Third-Party Licences** or [here](../../third-party-licenses.md).

### Is Manta available in English? {#sprache}

Yes. **Settings** → **General** → **Language**: **System** (the default, follows your preferred languages in macOS) or one of the app's seven languages: **Deutsch**, **English**, **简体中文**, **Español**, **Français**, **Italiano**, **Русский**. The switch applies immediately. The manual (**⌘?**) follows the app language too; if it isn't available in a language, Manta shows the English version.

### How many panes can I open? {#bereiche}

As many as fit in your window – each needs about 280 points of width. At least one always stays open. See [Panes & Tabs](02-bereiche-tabs.md).

## Files

### Where's the Trash? Can I get deleted files back? {#papierkorb}

Yes, for items on your Mac: **⌘⌫** moves them to the macOS Trash, like Finder, and Finder's **Put Back** restores them. **⌥⌘⌫** (**Delete Immediately…**) deletes permanently after asking. Servers, S3 and containers have no Trash: deleting there is always permanent, and Manta asks first. See [Delete](05-dateien.md#loeschen).

### Does dragging copy or move? {#ziehen}

Manta asks, with **Copy** preselected – also on the same volume, where Finder would move. That way nothing disappears from its folder by accident. Hold **⌘** when you let go to preselect **Move**; **⌥** preselects **Copy**. The badge at the pointer tells you beforehand what is preselected. In [Settings](16-einstellungen.md#kopieren-bewegen) you can choose "Always copy" or "Always move" (no dialog) instead. Dropping onto the Trash always moves to the Trash. See [Drag and drop](05-dateien.md#drag-drop).

### I opened a file from a server and changed it – where's my change? {#server-datei}

In a temporary copy on your Mac. Manta doesn't upload changed files automatically. Copy the file to a local folder, edit it there and copy it back with **F5**. See [Navigation](04-navigation.md#oeffnen).

### Why can't I see my `.env` or `.git` files? {#unsichtbar}

Files starting with a dot are hidden. Press **⇧⌘.**. See [Hidden files](04-navigation.md#unsichtbare-dateien).

### Why doesn't a folder update when another app adds files? {#aktualisieren}

Press **⌘R**. Manta re-reads folders after its own actions, but not when other apps change something.

### Can I change files inside ZIP archives? {#archiv-aendern}

No, archives are read-only. Extract them, change the files and compress them again (**⌥⌘C**). See [Archives](10-archive.md).

### What happens to a copy when Wi-Fi drops or I quit Manta? {#unterbrochen}

Nothing is lost. When the network drops, the transfer waits and continues at the same point as soon as the connection is back. When you quit Manta (or it crashes), it asks "Resume interrupted transfers?" at the next launch. See [Pause and resume transfers](05-dateien.md#pausieren).

### What are the `.NAME.manta-part` files? {#manta-part}

Hidden partial files Manta writes to while copying. Only the finished file gets its real name. See [Partial files](05-dateien.md#teildateien).

## Connections and cloud

### Are my server passwords stored securely? {#passwoerter}

Yes. With **Remember password**, Manta stores passwords, passphrases and S3 secret keys in the macOS Keychain, not in a file. Passwords that older versions saved unencrypted are moved there at launch. Without **Remember password**, Manta asks every time. SFTP with the ssh-agent needs no stored password at all. See [Connections](08-verbindungen.md#favoriten).

### Can I sign in to Google Drive directly? {#google}

No. Install **Google Drive for desktop** and sign in there; Manta then shows the Google Drive folder. See [Cloud Storage](09-cloud.md#google-drive).

### Does Manta support WebDAV, SMB or Dropbox without the app? {#protokolle}

Manta connects directly to SFTP, FTP, FTPS, S3 and containers. Mount SMB and WebDAV shares in Finder (**Go** → **Connect to Server**); they then appear under **Devices**. Manta shows Dropbox, OneDrive & co. through the providers' apps.

### Why is there a warning shield on the FTP chip? {#ftp-warnung}

Your network blocks encrypted FTP data connections. Manta therefore transfers files unencrypted; the login stays encrypted. See [Data channel](08-verbindungen.md#datenkanal).

### Can Manta start or stop containers? {#container}

No. Manta only shows and changes files in containers. Start and stop containers with Docker Desktop or in the terminal.

## Extending Manta

### Can I add my own commands without writing a plugin? {#eigene-befehle}

Yes, with **Quick Actions**: an entry with a shell command in `actions.json` is enough. See [Quick Actions](14-plugins.md#actions-json).

### Are plugins safe? {#plugins-sicher}

Built-in plugins are part of Manta. External plugins run with your user's permissions and without a sandbox – only install what you trust. See [Your Own Plugins](15-eigene-plugins.md#sicherheit).
