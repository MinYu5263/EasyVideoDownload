# 本地数据库

数据库由 Rust + rusqlite 管理，路径为 `local_data_dir()/EasyVideoDownload/app.db`。
使用 Tauri 的系统本地数据目录 API，不在数据库路径中添加 bundle identifier。

| 系统    | 数据库路径                                                                                        |
|---------|---------------------------------------------------------------------------------------------------|
| Windows | `%LOCALAPPDATA%\EasyVideoDownload\app.db`                                                         |
| macOS   | `~/Library/Application Support/EasyVideoDownload/app.db`                                          |
| Linux   | `$XDG_DATA_HOME/EasyVideoDownload/app.db`，未设置时使用 `~/.local/share/EasyVideoDownload/app.db` |

设置页“应用设置”卡片底部的“打开文件夹”按钮可直接打开 `app.db` 所在的应用数据目录。该入口仅在桌面端可用，设置读取失败时仍能使用。

## 当前结构（版本 3）

业务表只有 `app_settings` 和 `required_tools`。SQLite 自动维护的 `sqlite_schema`（兼容名称 `sqlite_master`
）保存表和索引定义，不由应用创建或删除。

### app_settings

每项设置一条记录：

| 字段        | 类型       | 用途                                              |
|-------------|------------|---------------------------------------------------|
| setting_key | TEXT，主键 | 稳定的设置名称                                    |
| value_json  | TEXT       | 合法 JSON，可表示字符串、布尔值、数字、数组或对象 |
| updated_at  | TEXT       | 本项设置最后变更的北京时间                        |

| 设置名称             | 默认 JSON 值 | 允许值                                       |
|----------------------|--------------|----------------------------------------------|
| locale               | `"zh-CN"`    | `"zh-CN"` / `"en"`；首次初始化可导入已有语言 |
| theme                | `"system"`   | `"system"` / `"light"` / `"dark"`            |
| notify_on_completion | `true`       | `true` / `false`                             |
| notify_on_failure    | `true`       | `true` / `false`                             |
| close_action         | `"ask"`      | `"ask"` / `"tray"` / `"exit"`                |

新增设置无需增加列：在 Rust 和前端补充定义、默认值、取值校验及界面即可。缺失的已知设置用默认值初始化，已保存的语言优先于首次导入值。未知键保留，保存已知设置不会删除它们。

JSON 格式由 SQLite 约束，具体设置的类型与允许值由 Rust 校验。已知设置类型或取值错误会返回加载错误，不用默认值覆盖。保存使用事务，只有变化项更新
`value_json` 和 `updated_at`；多项写入中途失败会全部回滚。

### required_tools

每个程序一条记录，以 `(tool_id, program_name)` 为复合主键。

| 字段            | 类型       | 用途                                     |
|-----------------|------------|------------------------------------------|
| tool_id         | TEXT       | `ytdlp` / `ffmpeg` / `deno`              |
| program_name    | TEXT       | `yt-dlp` / `ffmpeg` / `ffprobe` / `deno` |
| source          | TEXT       | `path` / `manual`                        |
| manual_path     | TEXT，可空 | 手动配置路径；PATH 来源为 NULL           |
| executable_path | TEXT       | 检测成功的程序绝对路径                   |
| version         | TEXT       | 检测到的程序版本                         |
| checked_at      | TEXT       | 实际检测完成的北京时间                   |

FFmpeg 与 FFprobe 共用工具组，两行中的来源、手动路径和检测时间必须一致。Rust
读取时检查程序是否完整及共享元数据是否一致；保存时整组替换，并在一个事务中提交。程序检测或保存失败继续保留原配置。

## 日期时间约定

所有业务时间字段使用 TEXT，统一采用北京时间（固定 UTC+8），格式为 `YYYY-MM-DD HH:mm:ss`，例如 `2026-10-03 14:33:51`，末尾不带时区后缀。
`HH` 为 24 小时制的小时，`mm` 为分钟，`ss` 为秒。时间生成不依赖操作系统的当前时区；Rust 写入和 SQLite 默认值均使用固定
UTC+8。后续 `created_at`、`completed_at` 等字段遵循同一约定。
工具接口的 `checkedAt` 同样为字符串。Rust 使用 chrono 生成和校验日期时间。

## 升级与兼容

`PRAGMA user_version` 是应用维护的结构版本：当前为 3。
`migrations/001_settings.sql` 保留第一版历史结构；`migrations/002_settings_key_value.sql` 将单行设置拆为键值记录、合并工具表，并将整数秒转换为
UTC 文本，保留原始时刻。
`migrations/003_beijing_datetime.sql` 将版本 2 的 UTC 文本转换为北京时间，并更新设置表的时间默认值。例如
`2026-10-03 06:33:51+00:00` 转为 `2026-10-03 14:33:51`，原始时刻不变，跨日时日期同时调整。升级成功后版本记为 3，重复启动不会再次转换。

建表、升级、旧 JSON 导入和版本更新位于同一个事务。升级前校验历史时间；无效日期或转换后超出四位年份范围会回滚原结构和数据，可修复数据后重试；高于
3 的未知版本会返回错误，不降级写入。
旧 `required-tools.json`、`program-dependencies.json`、`tools.json` 只在首次建库时导入并保留源文件；其整数秒时间和带
`+00:00` 的旧 UTC 文本在导入边界转换为北京时间，已经符合新格式的北京时间保持原值。

旧版本曾将数据库保存在 `app_local_data_dir()/EasyVideoDownload/app.db`，该 API 会自动添加 bundle identifier。新位置没有数据库时，使用
SQLite 在线备份导入旧文件（包含已提交的 WAL 数据），保留旧文件；新位置已有数据库时优先使用它，不自动覆盖。临时快照失败时清理自身文件，不留下空的正式数据库。

主题、通知和关闭行为当前只保存偏好，实际功能见 `TODO.md`。
