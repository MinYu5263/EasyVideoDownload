# 本地数据库

数据库由 Rust + rusqlite 管理，路径为 `local_data_dir()/EasyVideoDownload/app.db`。
使用 Tauri 的系统本地数据目录 API，不在数据库路径中添加 bundle identifier。

| 系统    | 数据库路径                                                                                        |
|---------|---------------------------------------------------------------------------------------------------|
| Windows | `%LOCALAPPDATA%\EasyVideoDownload\app.db`                                                         |
| macOS   | `~/Library/Application Support/EasyVideoDownload/app.db`                                          |
| Linux   | `$XDG_DATA_HOME/EasyVideoDownload/app.db`，未设置时使用 `~/.local/share/EasyVideoDownload/app.db` |

设置页“应用设置”卡片底部的“打开文件夹”按钮可直接打开 `app.db` 所在的应用数据目录。该入口仅在桌面端可用，设置读取失败时仍能使用。

## 当前结构（版本 11）

业务表包括 `app_settings`、`required_tools`、`download_page_states` 和 `download_records`。SQLite 自动维护的
`sqlite_schema`（兼容名称 `sqlite_master`
）保存表和索引定义，不由应用创建或删除。

### app_settings

每项设置一条记录：

| 字段        | 类型       | 用途                                              |
|-------------|------------|---------------------------------------------------|
| setting_key | TEXT，主键 | 稳定的设置名称                                    |
| value_json  | TEXT       | 合法 JSON，可表示字符串、布尔值、数字、数组或对象 |
| updated_at  | TEXT       | 本项设置最后变更的北京时间                        |

| 设置名称             | 默认 JSON 值             | 允许值                                              |
|----------------------|--------------------------|-----------------------------------------------------|
| locale               | `"zh-CN"`                | `"zh-CN"` / `"en"`；首次初始化可导入已有语言        |
| theme                | `"system"`               | `"system"` / `"light"` / `"dark"`                   |
| notify_on_completion | `false`                  | `true` / `false`                                    |
| notify_on_failure    | `true`                   | `true` / `false`                                    |
| close_action         | `"ask"`                  | `"ask"` / `"tray"` / `"exit"`                       |
| download_platform    | `"douyin"`               | `"douyin"` / `"bilibili"` / `"youtube"`             |
| settings_section     | `"application"`          | `"application"` / `"tools"` / `"proxy"` / `"about"` |
| active_page          | `"download"`             | `"download"` / `"history"` / `"settings"`           |
| proxy                | `null`                   | 全局代理的协议、地址、端口；`null` 表示未配置       |
| platform.douyin      | `{"proxyEnabled":false}` | 抖音的平台配置                                      |
| platform.bilibili    | `{"proxyEnabled":false}` | Bilibili 的平台配置                                 |
| platform.youtube     | `{"proxyEnabled":false}` | YouTube 的平台配置                                  |

新增设置无需增加列：在 Rust 和前端补充定义、默认值、取值校验及界面即可。缺失的已知设置用默认值初始化，已保存的语言优先于首次导入值。未知键保留，保存已知设置不会删除它们。

平台配置复用 `app_settings`，无需新增表或改变数据库版本。代理开关按平台独立保存，缺失时默认关闭，切换后立即保存；失败则恢复原状态。Cookie
继续存储在已有平台文件中。开启代理前必须已有合法全局代理；解析、下载与命令预览均在 Rust 中读取对应平台开关和全局代理的同一数据库快照。开关关闭时不传任何
`--proxy` 参数，也不改写子进程的代理环境，保留 yt-dlp 原有的系统代理、环境变量及 VPN 网络行为。开启时传入 `--proxy URL`
，同时令子进程代理环境与该 URL 一致并清除 `NO_PROXY`，避免不同网络后端绕过指定代理。开启但代理配置已被清空时返回错误，不静默直连。已开始的任务保留启动时的代理快照。

命令预览只显示 yt-dlp 程序路径与参数，不展开应用内部的进程环境设置。开启时显示 `--proxy URL`
，关闭时省略代理参数；复制到外部终端执行时使用该终端的环境，可能继承其默认代理。

JSON 格式由 SQLite 约束，具体设置的类型与允许值由 Rust 校验。已知设置类型或取值错误会返回加载错误，不用默认值覆盖。保存使用事务，只有变化项更新
`value_json` 和 `updated_at`；多项写入中途失败会全部回滚。

### required_tools

每个程序一条记录，以 `(tool_id, program_name)` 为复合主键。

| 字段            | 类型       | 用途                                     |
|-----------------|------------|------------------------------------------|
| tool_id         | TEXT       | `ytdlp` / `ffmpeg` / `deno`              |
| program_name    | TEXT       | `yt-dlp` / `ffmpeg` / `ffprobe` / `deno` |
| source          | TEXT       | `path` / `manual` / `automatic`          |
| manual_path     | TEXT，可空 | 手动配置路径；PATH 与自动配置来源为 NULL |
| executable_path | TEXT       | 检测成功的程序绝对路径                   |
| version         | TEXT       | 检测到的程序版本                         |
| checked_at      | TEXT       | 实际检测完成的北京时间                   |

FFmpeg 与 FFprobe 共用工具组，两行中的来源、手动路径和检测时间必须一致。Rust
读取时检查程序是否完整及共享元数据是否一致；保存时整组替换，并在一个事务中提交。程序检测或保存失败继续保留原配置。

工具页最近选择的来源、手动路径和检测结果保存在 `app_settings` 的 `tool_check.ytdlp`、`tool_check.ffmpeg`、`tool_check.deno`
键中，JSON 字段为 `source`、`manualPath` 和 `error`
。检测失败也会保存这份状态；重启时优先恢复最近的选择和错误，旧成功配置不会重新显示为当前检测结果。成功检测时，程序组和最近检测状态在同一事务内提交，并清除之前的错误。取消文件选择不会修改已保存的来源。

进入必备工具设置时会检查当前成功配置的程序文件是否仍存在，不启动程序；已删除的文件记录为 `notFound`
，隐藏旧版本与路径，保留内部成功配置用于后续重新配置。较新的失败选择不会被旧配置覆盖。视频下载启动前也检查 yt-dlp、FFmpeg 与
FFprobe 的实际文件；YouTube 使用已配置的 Deno 时同时检查该运行时。缺少工具时停止下载，保留下载页面并显示缺失提示；用户点击提示中的「去配置」按钮后打开必备工具设置。

自动配置的程序位于应用数据目录下的 `tools/yt-dlp`、`tools/ffmpeg` 和 `tools/deno`。FFmpeg 与 FFprobe
作为同一目录发布；下载、解压、验证或配置保存失败时保留原配置。

## 日期时间约定

所有业务时间字段使用 TEXT，统一采用北京时间（固定 UTC+8），格式为 `YYYY-MM-DD HH:mm:ss`，例如 `2026-10-03 14:33:51`，末尾不带时区后缀。
`HH` 为 24 小时制的小时，`mm` 为分钟，`ss` 为秒。时间生成不依赖操作系统的当前时区；Rust 写入和 SQLite 默认值均使用固定
UTC+8。后续 `created_at`、`completed_at` 等字段遵循同一约定。
工具接口的 `checkedAt` 同样为字符串。Rust 使用 chrono 生成和校验日期时间。

## 升级与兼容

`PRAGMA user_version` 是应用维护的结构版本：当前为 11。
`migrations/001_settings.sql` 保留第一版历史结构；`migrations/002_settings_key_value.sql` 将单行设置拆为键值记录、合并工具表，并将整数秒转换为
UTC 文本，保留原始时刻。
`migrations/003_beijing_datetime.sql` 将版本 2 的 UTC 文本转换为北京时间，并更新设置表的时间默认值。例如
`2026-10-03 06:33:51+00:00` 转为 `2026-10-03 14:33:51`，原始时刻不变，跨日时日期同时调整。升级成功后版本记为 3，重复启动不会再次转换。

`migrations/006_automatic_tools.sql` 将版本 5 的工具来源约束扩展到 FFmpeg 与 Deno，保留已有工具记录与其他表，升级成功后版本记为
6。

`migrations/007_single_input_link.sql` 删除页面表的 `parsed_link`，只保留 `input_link`
用于回显和对应结果的下载。旧输入与解析链接一致时保留结果，不一致时清除旧结果及选项，保留当前输入和保存目录；历史记录不变。数据调整、删列及版本更新在同一事务内完成，升级成功后版本记为
7。

`migrations/008_download_history_cards.sql` 给下载记录增加可空的 `error_stage` 和 `failure_kind`
，并增加状态分页索引。旧记录的失败阶段和分类保持未知，不根据旧文本推断补写；升级成功后版本记为 8。

版本 9 为下载记录增加可空的 `deleted_at` 和 `file_deleted_at` 及正常列表/回收站分页索引。旧记录保持原状态和快照，两项时间均为
NULL。移除记录只设置 `deleted_at`；恢复只清除该字段，保留原下载结果及文件曾回收的历史信息。

同次迁移保留已有记录的 ID，将主键改为 AUTOINCREMENT。永久删除或清空后不会重用已分配 ID，防止其他窗口持有的旧详情和确认操作指向新下载。

列表响应的 `fileRecyclingSupported` 和 `fileDeletionSupported` 由原生端分别提供。当前原文件回收和永久删除接入
Windows；其他系统禁用对应按钮并说明原因，仍可移入回收站、搜索及恢复记录。永久删除暂不使用 Unix 路径删除接口，避免并发替换文件名时误删其他文件。

建表、升级、旧 JSON 导入和版本更新位于同一个事务。升级前校验历史时间；无效日期或转换后超出四位年份范围会回滚原结构和数据，可修复数据后重试；高于
11 的未知版本会返回错误，不降级写入。
旧 `required-tools.json`、`program-dependencies.json`、`tools.json` 只在首次建库时导入并保留源文件；其整数秒时间和带
`+00:00` 的旧 UTC 文本在导入边界转换为北京时间，已经符合新格式的北京时间保持原值。

旧版本曾将数据库保存在 `app_local_data_dir()/EasyVideoDownload/app.db`，该 API 会自动添加 bundle identifier。新位置没有数据库时，使用
SQLite 在线备份导入旧文件（包含已提交的 WAL 数据），保留旧文件；新位置已有数据库时优先使用它，不自动覆盖。临时快照失败时清理自身文件，不留下空的正式数据库。

主题、通知和关闭行为已接入原生窗口、系统通知与托盘接口；原生失败会报告错误。

## 页面状态与下载记录

页面状态与下载记录通过 `migrations/005_persistence.sql` 接入，后续迁移保留在 `src-tauri/migrations/`
。建表、索引和版本更新在同一事务中完成，升级失败不写入部分结构。

`download_page_states`
每平台一行，只保存当前输入链接，以及该链接对应的真实元数据、格式顺序与精确选择、保存目录。输入变化或清空时清除可执行的旧结果与选项，界面暂时保留最近成功的封面与标题，新解析成功后替换；重新解析成功后才能从输入页开始下载；不另存解析链接。自定义空目录会保留。解析时间和工具指纹由
Rust 提供；工具配置变化会清除过期解析结果，同时保留输入和目录。Cookie 内容继续存储在平台文件中。

封面由原生 HTTP 接口限时下载到 `thumbnails/`，最大 5 MiB，读取校验缓存相对路径和文件位置。失败保留原始 URL 并报告错误。Vue
只将原生返回的图片字节显示为 Blob URL，替换和卸载时释放该 URL。

`download_records` 每个视频一行，以 `(platform, video_id)` 唯一识别。每次原生接收的下载尝试使用新 UUID，先保存 queued
状态，再执行准备与下载；准备失败与取消也结算到同一记录。缺少可靠解析快照或预检失败的请求不入队。历史快照独立于页面状态，成功文件的实际大小与所选流大小分别保存。失败根据原生代码、实际处理阶段及明确诊断分类，敏感诊断脱敏后保存。数据库结算失败通过
`storageError` 报告，实际文件和原始下载结果保留。

活动任务持有 `runtime/downloads/<UUID>.lock` 的原生跨进程锁；查询历史时只将能够取得锁的旧 running 记录恢复为
interrupted，其他实例仍在下载的记录保持 running。异常退出由操作系统释放锁。历史按开始时间和 ID 倒序游标分页，默认 50 条，上限
200。搜索标题和平台与状态过滤在整个数据库执行；列表、总数、匹配数及搜索范围内的状态统计使用同一事务快照。侧边栏不显示数量。

原生接口：`get_ui_preferences` / `save_ui_preferences`、`get_download_page_states` / `save_download_page_state`、
`cache_video_thumbnail` / `get_cached_thumbnail`、`create_download_request_id`、`list_download_records`。历史列表支持可选
`query` / `status` / `trashed`，继续支持 `cursor` / `limit`。`trashed` 缺省为 false；列表、匹配数和状态统计针对当前范围，
`trashCount` 为回收站总数，同一事务读取。

按记录 ID 操作的 `open_download_record_file`、`open_download_record_folder`、`open_download_record_source` 由 Rust
查询真实路径及验证目标后调用原生接口；来源仅支持 HTTP (S)，文件缺失时可打开仍存在的目录。`delete_download_record`
将终态记录移入应用回收站并保留文件；`restore_download_record` 恢复记录。`purge_download_record` 和
`empty_download_record_trash` 会彻底删除对应的最终视频文件，再永久删除回收站记录，不送入系统回收站。文件已不存在或记录没有最终输出时只删除记录。回收站没有自动清空或过期，列表仍按原下载时间排序和分组。

永久删除在 SQLite 写事务内重查回收站状态及活动下载，验证最终文件路径、普通文件类型和已记录大小；不会处理目录、下载片段或封面，仍被正常下载记录引用的文件也会保留。Windows
使用具有 DELETE 权限的文件句柄删除，禁止同时写入、替换或重命名，并再次检查句柄真实路径。不支持的平台原生端返回
`historyFileDeletionUnsupported`，不会回退到按路径删除。文件删除失败时保留对应记录；批量清空提交成功项、保留失败项并返回
`historyTrashPartiallyDeleted`，前端刷新列表供用户重试。文件已删除但数据库结算失败返回 `historyFileDeletedSaveFailed`
。下载进行中禁用永久删除和清空，原生端也再次检查；恢复记录仍可用。

详情页的 `delete_download_record_and_file`
只处理已完成记录的最终输出文件：在数据库写事务内再次检查记录和活动下载，校验路径属于原下载目录且为普通文件，再通过系统原生接口送入系统回收站。成功后记录进入应用回收站，
`file_deleted_at` 标记文件曾回收；文件原本缺失时只移除记录。恢复记录不会自动恢复文件。权限错误、原生回收失败或不安全路径均保留记录；文件已回收但历史保存失败返回
`historyFileRecycledSaveFailed`，明确部分成功。历史变化通知 `download-records-changed`
在数据库确认写入后发出，部分成功也触发刷新文件可用性；前端不自行插入下载记录。

## 共享下载任务与唯一视频记录（版本 10）

`download_records` 使用 `(platform, video_id)` 联合唯一约束，包含正常列表与回收站。解析和记录查询不会创建记录；原生接受开始下载的请求后插入
`queued` 条目，最多两个任务执行，其余按提交顺序排队。`request_id` 区分同一条目的不同尝试，旧请求的回调不能覆盖新任务。筛选「进行中」同时包含
queued 和 running。

每个任务在提交时固定目录、格式、代理、Cookie 副本与工具路径。实时进度通过 `download-task-changed` 事件和重连快照共享，逐帧进度不写入
SQLite；持久记录改变时发布 `download-records-changed`。排队任务也持有跨实例运行锁，刷新记录不会把仍在队列中的条目标为中断。

`output_path/output_extension/file_size_bytes` 与 `successful_*` 字段保存最后成功输出；失败或取消的新尝试保留旧输出。
`file_availability` 为
unknown/present/missing，解析时只查记录，文件打开时原生检查并更新。重新准备优先使用最后成功规格，不自动提交下载。托管工具文件正在被任务引用时禁止替换，普通工具来源切换作用于未来任务。

迁移 010 优先保留正常列表中最近成功的同视频记录；没有正常记录时在回收站中选取，再按最近尝试和 ID 排序。其余原始行归档至
`download_record_migration_archive`，不改动文件，保留 AUTOINCREMENT 高水位。所有结构变更和版本更新在一个事务内完成。

文件删除继续使用原生安全校验，排队与运行任务保守预留输出目录及旧成功路径。其他目录的记录操作可以继续；同一目标目录需等任务结束，以保护
yt-dlp 在执行时才确定的最终文件名。

下载页提交 `enqueue_video_download`
时携带当前已持久化的解析快照与重新下载意图。命中已有成功输出后，复用同一记录创建任务卡片，将安全删除原文件放在任务准备阶段；下载页使用本次选择的格式、画质、帧率与目录，刷新解析元数据，并保留原成功输出规格直到新任务成功。正在排队或运行的同视频仅返回现有任务。回收站条目仍需先确认恢复，接受前预检失败时保持回收站状态；成功接收时在同一事务恢复并更新任务。准备阶段删除失败时保留原文件，在正常列表中保留当前失败任务供重试。

记录内重新下载使用 `redownload_record(id, requestId)`
，原生层从数据库重建精确格式与保存目录，优先最后成功规格。工具、Cookie、代理及目录预检通过后，和下载页共用同一个入队与准备流程：复用同一记录更新任务
UUID，发布卡片，再在任务准备阶段以写事务重新校验原文件身份、共享路径、取消和活动冲突并安全删除。缺失文件直接重试；删除失败结算为当前任务失败，原文件不变，删除后保存失败返回
`historyFileDeletedSaveFailed`。下载页草稿不受影响。

下载页开始重复下载时，先复用同一记录入队并发布任务卡片；原文件保留到任务准备阶段，再重新验证工具、目标目录可写性、原文件身份和其它记录引用后删除。排队期间取消不会删除原文件，等待数据库锁期间取消也会在实际删除前检查。删除占用失败结算为当前任务失败，在卡片内显示“删除失败：文件被占用”，保留旧输出及其成功规格，关闭占用程序后可重试；下载记录卡片和详情内重下使用同一准备阶段删除流程，点击重下即关闭详情抽屉，错误只出现在卡片。原文件删除必须获取
Windows 独占删除句柄，拒绝仍被播放器或其他程序读取的文件，包括允许共享删除的读取句柄。共享冲突、锁冲突、等待删除和映射占用返回
`historyFileOccupied`；原生处置接口保留 NTSTATUS，`STATUS_CANNOT_DELETE` 在原文件非只读时识别为映射占用，只读与权限不足分别提示；仅凭
Win32 错误 5 不推断占用，后续输出确认阶段也保留该错误类型。

结构版本 11 增加 `output_identity`
，成功下载时保存文件身份、创建/修改时间与长度；彻底删除时用实际打开的文件句柄再次比对，拒绝同大小替换。系统回收也在操作前与原生删除回调中复查身份。旧记录没有原身份信息，保留
NULL 并沿用目录、路径、大小及占用检查。重新下载先入队，再在任务准备阶段删除原文件；不增加额外确认。
