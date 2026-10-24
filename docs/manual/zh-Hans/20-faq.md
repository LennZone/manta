# 常见问题 {#faq}

常见问题的简短解答。详细信息请参阅链接的章节。

## 通用

### Manta 能取代访达吗？ {#finder-ersetzen}

不能完全取代。你可以将 Manta 设为默认文件管理器；这样，“在访达中显示”以及（macOS 15 及更早版本中）来自其他 App 的文件夹都会在 Manta 中打开。但桌面、程序坞中的访达图标以及访达窗口中的连按操作始终由访达处理，macOS 不允许任何 App 接管它们。请参阅[默认文件管理器](01-erste-schritte.md#standard-dateimanager)。

### Manta 收费吗？会收集数据吗？ {#daten}

Manta 不收集任何使用数据。它只会为你自己的连接和检查更新而联网，检查更新时只发送你的 IP 地址和 Manta 版本。请参阅[隐私](17-updates.md#datenschutz)。

Manta 是免费的开源软件，采用 MIT 许可证：你可以自由使用、修改和分享。源代码在 [GitHub](https://github.com/web-workers-com/manta) 上（在 App 中可通过 **帮助** → **GitHub 上的源代码…** 打开），欢迎参与贡献。新闻、帮助和联系方式请访问网站 [manta-filemanager.com](https://manta-filemanager.com/)：在 App 中可通过 **帮助** → **Manta 网站与支持…** 打开，也可以在设置中的[关于 Manta](16-einstellungen.md#ueber)找到。

Manta 基于开源组件构建。它们的许可声明位于 **帮助** → **第三方许可**，或参阅[此处](../../third-party-licenses.md)。

### 我发现了错误或有新想法，应该在哪里报告？ {#feedback}

在 GitHub 上：**帮助** → **报告问题…** 会打开错误报告表单，并预先填好 Manta 和 macOS 版本；**帮助** → **提出建议…** 会打开想法表单。一份好的错误报告应包含什么、日志文件在哪里，请参见[报告错误](19-fehlerbehebung.md#fehler-melden)。安全漏洞请私下报告，切勿发到公开的 Issue 中，参见[安全政策](https://github.com/web-workers-com/manta/blob/main/SECURITY.md)。

### Manta 支持中文吗？ {#sprache}

支持。**设置** → **通用** → **语言**：**系统**（默认，跟随 macOS 中的首选语言）或 App 的七种语言之一：**Deutsch**、**English**、**简体中文**、**Español**、**Français**、**Italiano**、**Русский**。切换会立即生效。手册（**⌘?**）也会跟随 App 语言；如果没有该语言的版本，Manta 会显示英文版。

### 我可以打开多少个面板？ {#bereiche}

窗口能容纳多少就可以打开多少，每个面板大约需要 280 点的宽度。至少会始终保留一个面板。请参阅[面板与标签页](02-bereiche-tabs.md)。

## 文件

### 废纸篓在哪里？能找回已删除的文件吗？ {#papierkorb}

对于 Mac 上的项目可以：**⌘⌫** 会像访达一样将它们移到 macOS 的废纸篓，使用访达的 **放回原处** 即可恢复。**⌥⌘⌫**（**立即删除…**）会在询问后永久删除。服务器、S3 和容器没有废纸篓：在那里删除始终是永久性的，Manta 会先询问你。请参阅[删除](05-dateien.md#loeschen)。

### 拖移是拷贝还是移动？ {#ziehen}

Manta 会询问你，并预选 **拷贝**，即使在同一宗卷上也是如此（访达在这种情况下会移动）。这样就不会有项目意外从其文件夹中消失。松开时按住 **⌘** 可预选 **移动**；按住 **⌥** 则预选 **拷贝**。指针旁的标记会事先告诉你预选的是什么。你也可以在[设置](16-einstellungen.md#kopieren-bewegen)中改为选择“始终拷贝”或“始终移动”（不显示对话框）。拖放到废纸篓始终是移到废纸篓。请参阅[拖放](05-dateien.md#drag-drop)。

### 我打开了服务器上的文件并做了修改，我的修改在哪里？ {#server-datei}

在你 Mac 上的一个临时副本中。Manta 不会自动上传已更改的文件。请将文件拷贝到本地文件夹，在那里编辑，然后按 **F5** 拷贝回去。请参阅[导航](04-navigation.md#oeffnen)。

### 为什么我看不到 `.env` 或 `.git` 文件？ {#unsichtbar}

以点开头的文件是隐藏的。按 **⇧⌘.** 即可显示。请参阅[隐藏文件](04-navigation.md#unsichtbare-dateien)。

### 其他 App 添加文件后，文件夹为什么不更新？ {#aktualisieren}

按 **⌘R**。Manta 在执行自己的操作后会重新读取文件夹，但其他 App 更改内容时不会。

### 我可以更改 ZIP 归档中的文件吗？ {#archiv-aendern}

不可以，归档是只读的。请将其解压缩，更改文件后再重新压缩（**⌥⌘C**）。请参阅[归档](10-archive.md)。

### Wi-Fi 断开或我退出 Manta 时，正在进行的拷贝会怎样？ {#unterbrochen}

不会丢失任何内容。网络断开时，传输会等待，并在连接恢复后从中断处继续。如果你退出了 Manta（或 Manta 崩溃），下次启动时它会询问“要继续已中断的传输吗？”。请参阅[暂停和继续传输](05-dateien.md#pausieren)。

### `.NAME.manta-part` 文件是什么？ {#manta-part}

这是 Manta 在拷贝时写入的隐藏部分文件。只有完成的文件才会获得真正的名称。请参阅[部分文件](05-dateien.md#teildateien)。

## 连接和云端

### 我的服务器密码存储得安全吗？ {#passwoerter}

安全。打开 **记住密码** 后，Manta 会将密码、密码短语和 S3 私密密钥存储在 macOS 钥匙串中，而不是文件中。旧版本以未加密方式存储的密码会在启动时迁移到钥匙串。如果不打开 **记住密码**，Manta 每次都会询问。使用 ssh-agent 的 SFTP 则完全不需要存储密码。请参阅[连接](08-verbindungen.md#favoriten)。

### 我可以直接登录 Google Drive 吗？ {#google}

不可以。请安装 **Google Drive 桌面版** 并在其中登录；之后 Manta 就会显示 Google Drive 文件夹。请参阅[云存储](09-cloud.md#google-drive)。

### Manta 是否支持 WebDAV、SMB，或在没有其 App 的情况下支持 Dropbox？ {#protokolle}

支持。Manta 可直接连接 SFTP、FTP、FTPS、[WebDAV](08-verbindungen.md#webdav)、[SMB](08-verbindungen.md#smb)、S3 和容器——按 **⌘K**（**连接服务器…**）。在访达中装载的共享也会显示在 **设备** 下。Dropbox、OneDrive 等则由 Manta 通过服务提供商的 App 显示。

### 为什么 FTP 标签上有警告盾牌图标？ {#ftp-warnung}

你的网络阻止了加密的 FTP 数据连接。因此 Manta 以未加密方式传输文件；登录仍保持加密。请参阅[数据通道](08-verbindungen.md#datenkanal)。

### Manta 可以启动或停止容器吗？ {#container}

不可以。Manta 只显示和更改容器中的文件。请使用 Docker Desktop 或在终端中启动和停止容器。

## 扩展 Manta

### 不编写插件，可以添加自己的命令吗？ {#eigene-befehle}

可以，使用 **快速操作**：只需在 `actions.json` 中添加一个包含 Shell 命令的条目即可。请参阅[快速操作](14-plugins.md#actions-json)。

### 插件安全吗？ {#plugins-sicher}

内建插件是 Manta 的一部分。外部插件在 macOS 沙盒中运行，只能执行其权限允许的操作——不过仍请只安装你信任的插件。请参阅[自定插件](15-eigene-plugins.md#sicherheit)。
