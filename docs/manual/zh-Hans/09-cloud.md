# 云存储 {#cloud}

Manta 会显示你的 Mac 已经在同步的 iCloud Drive、Google Drive、OneDrive、Dropbox 和其他云存储。你可以看到哪些文件仅在云端，可以下载它们，也可以再次移除本地下载。

![边栏中的“云端”部分以及列表中的同步标记](images/cloud-sidebar-badges.png)

## Manta 如何访问云存储 {#prinzip}

Manta 本身 **不会** 登录 Google、Microsoft 或 Dropbox。它使用的是各提供商的官方 App 在你的 Mac 上创建的文件夹：

- **iCloud Drive** 内建于 macOS。
- **Google Drive**、**OneDrive**、**Dropbox**、**Box** 等服务，在你安装其 App 并登录后，会在 `~/Library/CloudStorage` 中创建各自的文件夹。

对 Manta 来说，这些都是 Mac 上的普通文件夹。你在 Manta 中所做的任何操作（拷贝、重新命名、删除），之后都会由提供商的 App 与云端同步。

## 设置 Google Drive {#google-drive}

1. 从 Google 下载并安装 **桌面版 Google Drive**。
2. 使用你的 Google 账户登录 Google Drive App。
3. **Google Drive** 会自动出现在边栏的 **云端** 下方：Manta 会注意到 `~/Library/CloudStorage` 中的新文件夹。如果没有出现，请点按 **云端** 旁边的圆形箭头。

OneDrive 和 Dropbox 的设置方式相同：安装提供商的 App，登录，完成。

> [!NOTE]
> Manta **没有自己的 Google 登录**。没有桌面版 Google Drive，你就无法在 Manta 中看到 Google Drive。

只要还没有 Google Drive 文件夹，**云端** 下方就会显示一行 **连接 Google Drive…**。点按它可以查看设置步骤以及链接 **获取桌面版 Google Drive**（<https://www.google.com/drive/download/>）。**不再显示** 会隐藏此提示；在 **设置 › 通用 › 云端** 中可以重新显示。

## 边栏中的“云端”部分 {#seitenleiste}

**云端** 部分位于 **位置** 和 **设备** 之间。找到的每个云存储都有自己的一行，并带有提供商的图标。如果某个提供商出现不止一次（两个 Google 账户、你公司的 OneDrive 和共享资源库），账户会以灰色显示在后面；否则，工具提示会连同路径一起显示账户：

- iCloud Drive（打开时始终排在第一位）
- Google Drive（有多个账户时：Google Drive *name@gmail.com*）
- OneDrive *个人* 或 OneDrive *你公司的名称*、OneDrive *共享资源库*
- Dropbox、Box 等（例如 Nextcloud、Proton Drive、Synology Drive）

点按即可在当前面板中打开该存储。

## 像在访达中一样的 iCloud Drive {#icloud-apps}

iCloud Drive 在 Manta 中的样子与在访达中相同：顶层文件夹会显示你的文件夹和文件，**以及**你的 App 的文件夹，例如 **Obsidian**、**Pages 文稿**、**Numbers 表格**或**快捷指令**。Manta 显示的 App 文件夹与访达相同（即在 iCloud Drive 中共享文稿且其中已有文稿的 App）。如果 App 已安装在你的 Mac 上，每个 App 文件夹都会显示该 App 的图标（设置 **使用 macOS 文件图标**）。

路径栏以 **iCloud Drive** 标签开头；在 App 文件夹中，后面跟着 App 的名称，例如 **iCloud Drive › Obsidian › Vault**。macOS 在磁盘上存放这些内容时使用的文件夹名称（`~/Library/Mobile Documents/com~apple~CloudDocs`、`iCloud~md~obsidian/Documents`）只有在你编辑路径时才会看到。

App 文件夹属于它的 App：你不能给它重新命名、移动或删除它；如果你尝试这样做，Manta 会告诉你。文件夹里的内容可以照常更改。

## 可用空间 {#freier-speicher}

在底部的状态栏中，项目数量后面会显示还剩多少可用空间，例如“12 项 · 可用 18 GB”。如果已知总容量，工具提示中也会显示。

| 位置 | “可用”的含义 |
|---|---|
| Mac 上的文件夹 | 磁盘的可用空间，与访达中相同，也就是包括 macOS 在需要时会释放的空间 |
| iCloud Drive | 你的 iCloud 储存空间中还剩余的空间 |
| Google Drive（自己的连接） | 你的 Google 存储空间中还剩余的空间；共享云端硬盘中不显示 |
| SFTP、SMB | 服务器上的可用空间 |
| WebDAV（例如 Nextcloud） | 你在服务器上的配额（如果服务器提供） |

桌面版 Google Drive、OneDrive 和 Dropbox 会将文件存放在你的 Mac 上；在这些位置，Manta 显示磁盘的可用空间。S3、FTP 和容器不提供可用空间信息，此时不显示此项。拷贝、移动或删除后，会重新读取该值。


## 标记的含义 {#abzeichen}

在云端文件夹中，Manta 会显示同步状态：在列表中显示为“名称”列右端的图标，在图标视图中显示为图标右上角的圆形标记。工具提示会用文字说明状态（例如“仅在云端，未下载”）。

| 标记 | 含义 | 可执行的操作 |
|---|---|---|
| （无） | 已下载且为最新 | 移除下载 |
| 带向下箭头的云 | 仅在云端，不在这台 Mac 上 | 立即下载 |
| 带感叹号的云 | 已下载，但云端有更新的版本 | 立即下载、移除下载 |
| 带向下箭头的圆圈 | 正在下载 | – |
| 带向上箭头的云（蓝色，闪烁） | 正在上传或正在等待上传，例如刚拷贝到 Google Drive 之后 | – |
| 圆圈中的红色感叹号 | 上传错误：提供商无法上传该项目；工具提示会给出原因（例如“上传错误：存储空间已满”） | – |

拷贝到云端文件夹后，提供商的 App 会在后台上传。上传期间，状态栏也会显示“Google Drive 正在上传 2 项”（出错时为红色）。iCloud Drive 还可能报告进度（工具提示中显示“正在上传… 42%”）；Google Drive、OneDrive 和 Dropbox 只报告“正在上传”。提供商从不上传的文件（`.DS_Store`、Office 锁定文件 `~$…`）没有标记。

读取状态不会下载任何内容，因此你可以浏览大型云端文件夹，而不会让 Mac 把所有内容都取回来。

Manta 从文件属性中读取状态（`SF_DATALESS` 标志以及 macOS 的 `NSURLUbiquitousItem…` 值，由 iCloud Drive 和 File Provider App 报告），并在文件夹可见时每隔几秒刷新一次。

## 从云端下载文件 {#jetzt-laden}

1. 选择仅在云端的文件或文件夹。
2. 右键点按 → **立即下载**。

文件夹会连同其中的所有内容一起下载。标记会变为“正在下载”，所有内容下载完成后标记会消失。

如果你打开或拷贝一个仅在云端的文件，macOS 也会自动下载它。

## 释放空间 {#download-entfernen}

1. 选择已下载的文件或文件夹。
2. 右键点按 → **移除下载**。

这些文件仍保留在云端，并在 Manta 中可见，但不再占用 Mac 上的空间。

> [!IMPORTANT]
> **移除下载** 对 **iCloud Drive** 可靠有效。对于 Google Drive、OneDrive、Dropbox 等，由提供商的 App 决定是否允许。如果它拒绝，Manta 会告诉你。这时请使用 **访达**：右键点按 → **移除下载项**。访达可以执行此操作，其他 App 则不行。

## 面向专家的 Google Drive：你自己的 OAuth 客户端 {#experte}

Manta 还包含一个自己的 Google Drive 连接（Drive API），但它是 **隐藏** 的：Manta 没有为它附带 OAuth 客户端。仅当你无法使用桌面版 Google Drive 并且拥有自己的客户端时：

1. 在 Google Cloud Console 中，创建一个类型为 **桌面应用** 的 OAuth 客户端，并启用 Google Drive API。
2. 打开 **设置 › 通用 › 云端 › 专家：你自己的 Google OAuth 客户端**，输入客户端 ID 和客户端密钥，然后点按 **存储**。
3. 现在“连接服务器”对话框中会有 **Google Drive** 类型。如果你将 **账户** 留空，Manta 会在连接时打开浏览器让你登录。

**移除** 会将 Google Drive 从“连接服务器”对话框中再次移除。客户端信息保存在 `~/Library/Application Support/Manta/gdrive-client.json` 中。

## 在云端文件夹中删除 {#loeschen}

> [!IMPORTANT]
> 云端文件夹是你 Mac 上的文件夹。**⌘⌫** 会像在其他位置一样，将其中的项目移到 **废纸篓**（请参阅[删除](05-dateien.md#loeschen)和[废纸篓](07-favoriten-seitenleiste.md#papierkorb)）；**⌥⌘⌫** 会在确认后永久删除。随后，提供商的 App 会将删除操作同步 **到云端和你的所有设备**。视提供商而定，该文件还会在其网站的废纸篓中保留一段时间。

如果你只是想节省空间，请使用 **移除下载**，而不是删除。

## 限制 {#grenzen}

- Manta 只显示你的 Mac 已经在同步的云存储。没有提供商的 App 就无法使用。
- 对于第三方提供商，**移除下载** 只在其 App 允许时可用（否则请在访达中操作）。
- 更改多快到达云端由提供商的 App 决定，而不是 Manta。
