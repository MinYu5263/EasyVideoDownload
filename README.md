# EasyVideoDownload

桌面端视频下载工具，使用 Tauri 2、Rust、Vue 3、TypeScript、Element Plus 和 SQLite。视频解析与下载由 Rust 调用
yt-dlp，FFmpeg/FFprobe 负责音视频处理，Deno 用于网站解析所需的 JavaScript 运行时。

## 使用方式

1. 在设置中检测必备工具，可使用系统 PATH、手动选择或支持平台上的自动配置。
2. 选择抖音、Bilibili 或 YouTube，粘贴视频链接或包含链接的分享文本。
3. 解析真实视频信息，选择画质、帧率和保存目录，再开始下载。
4. 在下载卡片和历史记录中查看进度、取消任务、重新下载或打开保存的文件。

每条链接只处理一个视频；最多同时执行两个下载，其余按提交顺序排队。下载格式使用所选视频流的精确 ID，有独立音频时交给 FFmpeg
合并。画质和可访问内容取决于网站、Cookie 及账号权限。

Cookie 支持导入 Netscape 格式 UTF-8 文件、原生剪贴板粘贴及直接编辑，按平台独立保存在本机。修改内容自动保存，解析和下载使用临时副本，不让
yt-dlp 回写用户维护的文件。具体行为见 [Cookie 说明](docs/cookies.md)。

设置、页面状态和下载历史由 Rust 写入 SQLite；实时进度来自下载进程。应用提供浅色/深色/系统主题、下载通知、关闭询问与系统托盘功能。剪贴板、目录选择、文件操作、外部链接和通知均通过桌面原生接口执行。

## 开发与构建

需要 Node.js、pnpm 和 Rust。Windows 还需要 C++ 构建工具及
WebView2；其他平台需要对应系统构建依赖，参见 [Tauri 环境要求](https://v2.tauri.app/start/prerequisites/)。

首次准备项目：

```powershell
pnpm install --frozen-lockfile
pnpm icons
pnpm tauri dev
```

常用检查：

```powershell
pnpm exec vue-tsc --noEmit
pnpm build
cargo check --manifest-path src-tauri/Cargo.toml --locked
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

`pnpm dev` 和 `pnpm preview` 用于界面预览，浏览器中不能执行真实解析、下载和系统功能。本地前端回归通过 `pnpm test` 执行；
`tests/` 为 Git 忽略的本地验证内容，不随仓库分发。Rust 测试随业务代码维护，默认跳过需要真实窗口、通知、系统回收站或外部网络的手动烟测。

发布安装包：

```powershell
pnpm icons
pnpm tauri build
```

安装包生成在 `src-tauri/target/release/bundle/`。请在目标操作系统上构建和验证；构建成功不能替代真实网站下载、文件播放、系统通知及安装后的桌面验证。

## 图标与目录

主图标维护在 `src/assets/app-icon.svg`，界面直接使用 SVG。`pnpm icons` 生成系统 PNG、ICO 和 ICNS，保存在忽略的
`src-tauri/target/generated-icons/`。首次构建、清理 Cargo 目录或更新 SVG 后应重新生成；Cargo、Vite 不会自动生成图标。

| 路径                         | 用途                                       |
|------------------------------|--------------------------------------------|
| `src/`                       | Vue 页面、共享状态、IPC 调用和本地化       |
| `src-tauri/src/`             | 原生命令、任务调度、文件安全检查和数据存储 |
| `src-tauri/migrations/`      | 数据库升级脚本，必须保留已有版本           |
| `src-tauri/capabilities/`    | 桌面权限配置                               |
| `scripts/generate-icons.mjs` | 桌面图标生成                               |
| `docs/`                      | Cookie 与数据库正式说明                    |

Windows 用户数据位于 `%LOCALAPPDATA%\EasyVideoDownload\`；其他系统通过 Tauri 的 `local_data_dir()` 定位。数据包括 `app.db`
、Cookie、封面缓存及托管工具。设置中的“打开文件夹”可打开数据目录。迁移和历史文件安全规则见 [数据库说明](docs/database.md)。

## 平台与功能限制

- 工具自动配置支持 Windows、macOS 的相应架构；Linux 使用系统 PATH 或手动路径。yt-dlp、Deno 使用官方发布源，FFmpeg 使用
  Gyan（Windows）和 Evermeet（macOS）。Apple 芯片上的自动 FFmpeg 需要 Rosetta 2。工具更新管理尚未接入。
- 当前支持取消和重新下载，未提供暂停/恢复、合集或播放列表下载。
- 系统回收站与安全永久删除仅在 Windows 接入。macOS/Linux 可以移除或恢复历史记录，存在旧输出文件的重新下载会因安全删除暂不支持而报错。
- 同一视频复用历史记录；重新下载在入队后、实际任务准备时安全删除旧输出。取消排队任务会保留旧文件。文件占用、身份变化或被其他记录引用时拒绝删除。
- 历史记录的应用回收站与系统回收站相互独立；恢复记录不会恢复已删除的视频文件。
- 平台解析能力由网站和 yt-dlp 决定，需以实际生成的视频、音轨和所选规格验收。
