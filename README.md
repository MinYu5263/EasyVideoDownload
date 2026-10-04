# EasyVideoDownload

基于 Tauri 2、Vue 3、TypeScript、Element Plus 和 SQLite 的桌面端视频下载工具，由 Rust 调用 yt-dlp 完成视频解析与下载。

**当前版本：v0.1.0 · Early / Testing（第一版测试版本）**

项目仍处于早期测试阶段，尚未达到稳定正式版。部分功能可能不完善，使用中可能遇到 Bug 或系统兼容性问题，后续版本也可能调整功能和交互。欢迎通过
Issue 反馈问题、提出建议，也欢迎开发者提交 Pull Request，一起完善项目。

## 已实现的核心功能

- **视频解析与下载**：提供抖音、Bilibili 和 YouTube 入口，支持从分享文本提取链接，展示真实标题、封面和可用格式，选择画质、帧率及保存目录；独立音视频流通过
  FFmpeg 合并。
- **下载任务**：最多同时执行两个下载，其余按提交顺序排队；显示实际进度、速度及处理阶段，支持取消和重新下载，同一视频复用历史记录。
- **历史管理**：支持搜索、状态筛选、详情查看，打开文件、所在文件夹和来源链接；可将记录移入应用回收站或恢复，Windows
  另支持文件回收与安全永久删除。
- **必备工具配置**：检测系统 PATH、手动选择工具路径，或在支持的平台自动配置 yt-dlp、FFmpeg/FFprobe 和 Deno；自动配置提供进度反馈及取消操作。
- **Cookie 与代理**：按平台导入 Netscape 格式 UTF-8 Cookie 文件、粘贴或编辑并自动保存；支持 HTTP、HTTPS、SOCKS5
  代理配置、连接测试和平台独立开关。
- **桌面设置与数据保存**：提供中文/英文界面、浅色/深色/系统主题、下载通知、关闭询问和系统托盘；本地保存设置、页面状态、下载历史及封面缓存。
- **命令预览**：查看并通过原生剪贴板复制解析、下载所用的 yt-dlp 命令。

上述能力已在代码中接入，但网站解析及下载结果仍受 yt-dlp、网站规则、Cookie 和账号权限影响。

## 项目截图

以下为 Windows 浅色主题下的实际界面。

### 视频解析与下载

![视频解析与下载页面](docs/screenshots/download.png)

### 下载记录

![下载记录页面](docs/screenshots/history.png)

### 必备工具配置

![必备工具配置页面](docs/screenshots/tools.png)

## 下载安装与使用

测试版本及可下载的安装附件以 [Releases](https://github.com/MinYu5263/EasyVideoDownload/releases)
中的实际发布内容为准。选择与你的操作系统和架构匹配的安装包，按照该版本说明安装后启动 EasyVideoDownload。Windows 的 `.msi` 或
`-setup.exe` 安装包可直接运行安装；没有对应安装包时，可按下文从源码构建。

首次使用需要配置下载工具：yt-dlp 负责解析与下载，FFmpeg/FFprobe 用于音视频处理，Deno 用于 YouTube 解析所需的 JavaScript
运行时。工具不必预先加入系统 PATH，也可以在应用内手动配置或使用支持平台的自动配置。

1. 启动程序，进入「设置 → 必备工具」，配置并检测所需工具。手动配置 FFmpeg 时，应确保所选目录包含 FFmpeg 和 FFprobe。
2. 在下载页选择抖音、Bilibili 或 YouTube，粘贴视频链接或包含链接的分享文本。
3. 如需访问登录内容，在当前平台配置中导入或编辑 Cookie；如需代理，先在设置中配置代理，再开启当前平台的代理开关。
4. 解析视频，确认标题、画质、帧率和保存目录后开始下载。
5. 在任务卡片查看进度或取消任务，完成后通过历史记录打开文件或文件夹；失败时根据提示调整配置后重试。

Cookie 按平台独立保存在本机。解析与下载使用临时副本，避免 yt-dlp 回写用户维护的 Cookie
文件。详细行为见 [Cookie 说明](docs/cookies.md)。

## 开发环境

- **Node.js**：满足当前锁定的 Vite 依赖要求：`^20.19.0 || >=22.12.0`。
- **pnpm**：项目使用 `pnpm-lock.yaml` 管理依赖。
- **Rust 与 Cargo**：用于编译原生后端。
- **Tauri 2**：CLI 已作为项目开发依赖提供，通过 `pnpm tauri` 调用。
- **系统构建依赖**：Windows 需要 C++ 构建工具和 WebView2；macOS 需要 Xcode Command Line Tools；Linux 需要
  WebKitGTK、编译工具等对应发行版依赖。具体安装方式见 [Tauri 环境要求](https://v2.tauri.app/start/prerequisites/)。

SQLite 通过 Rust 依赖内置，无需单独部署数据库服务。开发期间进行真实解析和下载时，也需要配置上述下载工具。

## 本地开发与构建

在项目根目录安装依赖、生成桌面图标并启动 Tauri 开发窗口：

```sh
pnpm install --frozen-lockfile
pnpm icons
pnpm tauri dev
```

`pnpm dev` 和 `pnpm preview` 可用于前端界面预览；真实解析、下载、文件操作等功能需要在 Tauri 桌面窗口中执行。

常用检查与前端构建：

```sh
pnpm exec vue-tsc --noEmit
pnpm build
cargo check --manifest-path src-tauri/Cargo.toml --locked
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

本地前端测试通过 `pnpm test` 执行，但 `tests/` 是被 Git 忽略的本地验证内容，克隆仓库时不包含这些文件。Rust
测试随源码维护，默认跳过需要真实窗口、系统通知、系统回收站或外部网络的手动烟测。

构建桌面安装包：

```sh
pnpm icons
pnpm tauri build
```

Tauri 构建会自动执行 `pnpm build`，安装包输出到 `src-tauri/target/release/bundle/`
。请在目标操作系统上构建并验证；构建成功不能替代实际安装、网站下载和文件播放测试。

主图标维护在 `src/assets/app-icon.svg`。`pnpm icons` 生成 PNG、ICO 和 ICNS，保存至被忽略的
`src-tauri/target/generated-icons/`；首次构建、清理 Cargo 构建目录或更新 SVG 后需重新生成，Cargo 和 Vite 不会自动生成桌面图标。

## 项目结构

| 路径                      | 用途                                            |
|---------------------------|-------------------------------------------------|
| `src/`                    | Vue 页面、共享状态、IPC 调用、本地化和界面资源  |
| `src-tauri/src/`          | Rust 原生命令、任务调度、文件安全检查和数据存储 |
| `src-tauri/migrations/`   | SQLite 数据库升级脚本，保留历史版本以支持迁移   |
| `src-tauri/capabilities/` | Tauri 桌面权限配置                              |
| `scripts/`                | 桌面图标生成脚本                                |
| `docs/`                   | Cookie 与数据库说明                             |

## 本地数据说明

设置、页面状态和下载历史由 Rust 写入本地 SQLite 数据库 `app.db`。Cookie 文件、封面缓存和自动配置的工具也保存在应用数据目录，下载的视频保存在用户选择的目录。

数据目录由 Tauri
的系统路径接口定位，可通过设置中的「打开文件夹」查看。数据库包含版本迁移，升级失败会回滚；结构与迁移细节见 [数据库说明](docs/database.md)。

## 当前版本已知限制

- 当前只处理单个视频，不支持合集、播放列表或多段视频结果；下载支持取消和重新下载，尚未提供暂停/恢复。
- 工具自动配置仅覆盖 Windows、macOS 的部分架构，以工具卡片是否提供该选项为准；Linux 使用系统 PATH 或手动路径。yt-dlp、Deno
  使用官方发布源，FFmpeg 使用 Gyan（Windows）和 Evermeet（macOS）。Apple 芯片上的自动 FFmpeg 需要 Rosetta 2，工具更新管理尚未接入。
- 系统回收站与安全永久删除仅在 Windows 接入。macOS/Linux 可移除或恢复历史记录，但存在旧输出文件的重新下载会因安全删除暂不支持而报错。
- 重新下载会在任务实际准备时安全删除旧输出；取消排队任务保留旧文件。文件被占用、身份变化或被其他记录引用时会拒绝删除。
- 应用回收站与系统回收站相互独立；恢复历史记录不会自动恢复已删除的视频文件。应用回收站的永久删除/清空会删除对应最终视频文件，不送入系统回收站。
- 网站变化、工具版本和账号权限可能影响解析与下载，v0.1.0 也可能存在尚未发现的问题。

## Roadmap

后续优先方向：

- 根据使用反馈修复 Bug，改善下载失败提示与交互体验。
- 补充不同系统环境的测试，完善跨平台文件操作与兼容性。
- 持续完善文档和回归验证，逐步提高版本稳定性。

## 参与贡献

这是项目的第一个版本，还有不少地方可以改进。无论是使用中的问题、更顺手的交互方案，还是功能建议，都欢迎通过 [Issue](https://github.com/MinYu5263/EasyVideoDownload/issues)
分享。

如果愿意参与开发，欢迎 Fork 项目并提交 [Pull Request](https://github.com/MinYu5263/EasyVideoDownload/pulls)。Bug
修复、体验优化、文档完善，以及不同操作系统上的测试反馈，都是很有帮助的贡献。较大的改动可以先通过 Issue
讨论需求和实现方向；协作约定见 [AGENTS.md](AGENTS.md)。

## 问题反馈

请通过 [Issue](https://github.com/MinYu5263/EasyVideoDownload/issues) 提交问题，尽量提供：

- 应用版本、操作系统及架构，相关下载工具的版本。
- 涉及的平台、操作步骤、预期结果与实际结果。
- 可复现的错误信息或真实截图。

提交前请移除 Cookie、带令牌的链接等账号信息，并对个人路径等隐私内容脱敏。

## License

当前仓库未包含 LICENSE 文件，暂未声明许可证。
