# 自定插件 {#eigene-plugins}

你可以用自己的插件扩展 Manta，并且可以使用任何编程语言编写。本章介绍插件能做什么、如何安装、更新和查找插件，以及需要注意什么。技术指南请参阅开发者文档。

## 插件能做什么 {#moeglichkeiten}

- 在文件名旁边显示**标记**（类似 Git 标记）。
- 在路径栏中显示**标签**（类似分支或项目标签）。
- 为命令面板和快捷菜单提供**命令**，并可按需配备键盘快捷键。
- 在面板下方、右侧边栏中或以对话框形式显示**面板**。
- 在**状态栏中显示项目**，带有图标、小进度条和警告颜色（如[系统负载](14-plugins.md#systemauslastung)）。
- 显示**通知**。

插件是一个包含 `plugin.toml` 文件和一个程序（例如 Python 脚本）的文件夹。Manta 会启动该程序，并与它交换消息（通过 stdin/stdout 传输的 JSON 行）。`.mantaplugin` 文件就是将该文件夹压缩而成的 ZIP 归档。

## 安装插件 {#installieren}

1. 打开**设置**（**⌘,**）→ **插件**。
2. 点按**安装插件…**，然后选择**从文件安装（.mantaplugin、.zip）…**或**从文件夹安装…**（包含 `plugin.toml` 的文件夹）。更快的方法：在访达中**连按** `.mantaplugin` 文件，或将它**拖到**“插件”页面上。社区插件目录中的插件可通过[发现](#entdecken)安装。
3. 在安装任何内容之前，Manta 会显示一份概览：名称、版本、作者、描述、**允许的操作**（例如“读取你的文件”“更改或删除文件”“使用网络”“运行其他程序”“在终端中运行命令”）、运行的**命令**、**来源**、**源代码**链接、**许可**和大小。下方还会提醒你 Manta 不会检查插件。
4. 点按**安装**。之后插件会列在**已安装**下，但仍处于关闭状态。

![安装插件前的概览](images/plugin-install.png)

> [!NOTE]
> 权限由插件作者声明，而 Manta 会**强制执行**这些权限：插件在 macOS 沙盒中运行，无法执行其他操作（请参阅[安全性](#sicherheit)）。

### 打开插件 {#einschalten}

点按插件卡片上的**打开**。首次打开时，Manta 会请你确认，并再次显示作者、版本、权限、命令以及运行它的文件夹。

![打开外部插件时的确认](images/plugin-confirm.png)

如果插件是从互联网下载的，macOS 会将其文件隔离，Gatekeeper 会阻止其中的程序运行。确认窗口会提示这一点：只有在你打开插件时，Manta 才会移除其文件上的隔离标记，让其中的程序能够运行——绝不会在你不知情时这样做。

插件卡片上的其他按钮：**重新载入**、**显示日志**、**显示文件夹**、**卸载**。顶部还有：**打开插件文件夹**和**重新载入**（查找新增、更改或移除的插件）。

## 更新和卸载 {#aktualisieren}

- **更新**：如果插件目录中有更新的版本，插件卡片上会显示“插件目录中有版本 …”和**更新**按钮。你也可以用同样的方式（从文件或文件夹）安装新版本。如果它与已安装的插件 ID 相同，概览标题会显示为更新，并在**已安装**下显示将被替换的版本。确认后，Manta 会替换该插件，并保留其打开/关闭状态。
- **卸载**：点按插件卡片上的**卸载**。Manta 会删除插件文件夹；其数据文件夹会保留。

## 查找插件 {#entdecken}

在**设置** → **插件**中，点按**发现**（或在命令面板中选择**浏览插件…**）。Manta 会显示社区插件目录中的插件：以你的语言显示的名称和描述、作者、许可、所需的 Manta 版本、权限以及源代码链接。

1. 点按**安装**（如果已安装的插件有更新的版本，则点按**更新**）。
2. Manta 会下载插件，并将其 SHA-256 校验和与目录中的校验和进行比较。如果两者不一致，则不会安装任何内容。
3. 随后会显示常规概览（“SHA-256 与插件目录一致”）；接下来按[上文](#installieren)所述操作。

需要更新版本的 Manta 或无法在你的 Mac 上运行的插件也会显示，但无法安装。**在网页上查看**会打开 [manta-filemanager.com/plugins](https://manta-filemanager.com/plugins/) 上的插件目录。

> [!NOTE]
> **隐私：**只有当你打开**发现**（每次会话一次）或点按**刷新**时，Manta 才会从 GitHub 载入列表——绝不会在后台载入。Manta 会保留上一次的列表，因此“发现”在离线时也能使用，已安装的插件无需联网也能提示更新。

## 安全性 {#sicherheit}

外部插件在 **macOS 沙盒**中运行：它们只能执行其权限允许的操作。没有任何权限的插件只能读取它自己的文件夹和系统文件，并写入它自己的数据文件夹。

| 权限 | 允许插件 |
|---|---|
| 读取你的文件 | 读取任何文件 |
| 更改或删除文件 | 在任何位置写入、创建和删除文件 |
| 使用网络 | 连接互联网和你的网络 |
| 运行其他程序 | 启动其他程序（这些程序仍在沙盒中） |
| 在终端中运行命令 | 在 Manta 的终端中输入命令 |

插件请求 Manta 代为执行的操作（读取文件、列出文件夹内容、重新命名文件）也适用同样的规则。插件卡片上会显示**沙盒**；其工具提示会列出各项权限。内建插件是 Manta 的一部分，不在沙盒中运行。

如果沙盒在你的 Mac 上无法工作，卡片会显示**未使用沙盒**及原因，概览中也会提示无法强制执行权限。此时插件将以你的用户权限运行。

> [!WARNING]
> 沙盒并不能让插件变得无害：一个可以读取你的文件并使用网络的插件，可以把你的文件发送到别处。Manta 不会检查插件——插件目录中的插件也是如此。请只安装你信任的插件；对于来自互联网的插件，请在打开之前阅读其代码。

因此，Manta 会在安装前显示概览，新插件默认处于关闭状态，并且你首次打开插件时 Manta 会先询问。

## 插件无法运行时 {#probleme}

- 卡片会显示**错误**及其原因，例如缺少 `python3` 时。
- **显示日志**会显示插件最近的消息。
- 如果插件崩溃，Manta 会重新启动它，但每分钟最多三次。超过之后，它会保持错误状态，直到你点按**重新载入**。
- 如果插件文件夹中有两个插件 ID 相同的文件夹，第二个会显示**无法载入**和一条消息。请移除其中一个。

## 插件的存储位置 {#speicherorte}

| 内容 | 位置 |
|---|---|
| 已安装的插件 | `~/Library/Application Support/Manta/plugins/<id>/` |
| 插件的数据和日志 | `~/Library/Application Support/Manta/plugin-data/<id>/`（日志：`plugin.log`） |
| 打开/关闭状态 | `~/Library/Application Support/Manta/plugins.json` |
| 上一次的插件目录列表 | `~/Library/Application Support/Manta/plugin-registry.json` |
| 插件的沙盒规则 | `~/Library/Application Support/Manta/plugin-data/<id>/sandbox.sb` |

卸载会删除插件的文件夹；其数据文件夹会保留。

## 自己编写插件 {#schreiben}

最快的入门方式是 **Hello Python** 示例（源代码中的 `examples/plugins/hello-python`，逐行注释，仅使用 Python 标准库）。你也可以通过**设置** → **插件** → **可用**安装它。此外还有 Node.js、Shell（bash + jq）和 Rust 模板（`examples/plugins/`）；Rust 还可以使用 `manta-plugin-protocol` crate。

指南、协议参考和示例：[编写 Manta 插件（docs/plugins.md）](../../plugins.md)。

插件可以支持 Manta 的所有语言：Manta 会告诉插件当前语言，`plugin.toml` 中也可以包含翻译后的名称和描述（例如 `[localized.zh-Hans]`、`[localized.de]`）。

## 开发和测试 {#entwickeln}

- **重新载入插件**（命令面板）会重新启动所有正在运行的外部插件，并重新扫描插件文件夹。
- **开发者模式**：在终端中这样启动 Manta：
  `/Applications/Manta.app/Contents/MacOS/manta --plugin-dev ~/code/my-plugin`
  （可重复指定，也可以设置环境变量 `MANTA_PLUGIN_DEV=dir1:dir2`）。在本次会话中，插件会直接从该文件夹运行——视为可信、已打开，并带有**开发**标签。期间已安装的同 ID 副本会暂时让位；不会保存任何内容。
- **不启动 Manta 进行测试**：`python3 scripts/plugin-test.py path/to/plugin`。
- **打包**：`python3 scripts/plugin-pack.py path/to/plugin` 会生成 `.mantaplugin` 文件，并输出用于插件目录的 SHA-256 校验和。
- **沙盒**：声明你的插件所需的权限——没有声明的权限，插件就无法执行相应操作。如果某项操作被拒绝，日志中会记录下来（`refused …`）。
- **日志**：卡片上的**显示日志**，或文件 `~/Library/Application Support/Manta/plugin-data/<id>/plugin.log`。

更多信息：[Develop and debug（docs/plugins.md，英文）](../../plugins.md#develop-and-debug)。

## 发布插件 {#veroeffentlichen}

将插件放在使用 OSI 认可许可证的公开仓库中，在某个 Release 中附上 `.mantaplugin` 文件，然后提交一个拉取请求，把它添加到 `plugins/registry.json`。之后它就会出现在**发现**中。详情：[Publishing a plugin（docs/plugins.md，英文）](../../plugins.md#publishing-a-plugin)。
