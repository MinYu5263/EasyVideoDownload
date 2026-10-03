# SQLite 设置结构第二版实施计划

> 执行方式：使用 executing-plans 在当前工作区实现；用户已批准设计和实施，不执行 Git 状态变更。

> 此文保留版本 2 的设计与执行记录。后续按用户要求升级为版本 3，所有业务时间改用北京时间（固定 UTC+8），格式为
> `YYYY-MM-DD HH:mm:ss`，不带时区后缀。当前约定见 [本地数据库](../database.md)。

**目标**：设置改为键值记录，合并必备工具表，所有业务时间使用可读 UTC 日期时间，并无损升级已有数据库。

**设计依据**：本次对话确认 `app_settings(setting_key, value_json, updated_at)`；`required_tools` 每个程序一行，以
`(tool_id, program_name)` 为主键；时间格式 `YYYY-MM-DD HH:MM:SS+00:00`。

**架构**：保留版本 1 SQL，新增版本 2 事务迁移。Rust 保持类型明确的设置接口及默认值校验，数据库允许新增键；仅更新变化的已知设置，保留未知设置。Rust
将工具行还原为完整工具组并校验共享元数据；FFmpeg/FFprobe 整组保存。

## 约束与文件

- 数据库现使用 `local_data_dir()/EasyVideoDownload/app.db`，按后续用户要求去除应用标识目录；设置页面行为保持一致。
- `src-tauri/migrations/001_settings.sql` 保持历史格式；新增 `002_settings_key_value.sql` 转换旧数据和时间。
- 调整 `src-tauri/src/database.rs` 的升级、设置读写、工具组读写；保留失败回滚和未知新版保护。
- 调整 `src-tauri/src/required_tools.rs`、`src/composables/useRequiredTools.ts`，将检测时间改为字符串；旧 JSON
  的整数秒仍可导入，源文件保留。
- 使用已在依赖树中的 chrono 进行日期时间生成与有效性校验，新增直接依赖及日期时间模块。
- 新增或调整 Rust 测试；独立前端测试保留在已忽略的 `tests/`，不纳入提交。
- 不接入主题、通知或托盘实际功能，不运行用户真实数据库迁移，不执行 git add/commit/push。

## 任务与验证

- [x] 先添加并运行失败测试：版本 1 的设置和四个程序升级后可恢复，时间保留原始时刻；缺项默认值、未知键保留、只更新变化项的时间、整组写失败回滚、升级失败保留旧结构且可重试。
- [x] 实现版本 2 SQL 和事务升级，设置键值读写及合并工具读写；运行数据库测试。
- [x] 调整检测时间和旧 JSON 兼容转换、前端类型及本地测试数据；运行 Rust/前端全部测试。
- [x] 更新持久化文档，运行 pnpm build、cargo fmt --check、cargo clippy --all-targets -- -D warnings，并审查工作区差异。

## 审查重点

1. 旧表为空或设置尚未初始化时仍可升级；初始化旧语言仅用于缺少 locale 的情况。
2. 数据异常导致迁移失败时，原表和 user_version 保留。
3. 未知设置不丢失；已知设置 JSON 格式合法但类型或取值错误时不写默认值覆盖。
4. 合并表中的 FFmpeg/FFprobe 元数据一致，缺程序与不一致元数据返回加载错误。
5. 所有新时间为固定 UTC 文本；仅历史迁移边界接受整数秒，不变更其所指时刻。

## 执行记录

- 开始状态：只有 `SettingsPage.vue` 的版本导入修复未提交，保留该改动。
- 接口检查：前端仍使用完整的 AppSettings 类型；数据库以已知键逐项 upsert，不需要改变页面保存队列。工具 checkedAt 从 number
  改为 string，旧数据仅在迁移边界转换。
- RED：数据库新行为测试因缺少键值结构、日期时间转换和迁移失败保护而失败；整数旧 JSON 时间转换测试也失败。
- GREEN：数据库 18 项通过；Rust 完整测试 33 项通过、1 项既有进程辅助测试忽略；前端 14 项通过，pnpm build 通过。
- 前端测试首次受 pnpm 依赖路径的沙箱 EPERM 限制，沙箱外重新运行通过。
- 验证：cargo fmt --check、cargo clippy --offline --all-targets -- -D warnings、git diff --check 通过。Git 中原有 version
  修复保留；没有暂存或提交。
- 独立只读审查：无 Critical / Important / Minor 问题；额外用内存 SQLite 验证空版本 1 表、整数秒边界转换及超范围值失败回滚。
- 范围确认：主题、通知和托盘效果按用户要求留待后续；外部手写 SQL 的任意日期文本不增加额外校验，正常应用写入统一生成 UTC 文本。
- 升级仅在临时测试数据库验证；用户实际数据库将在下次应用启动时自动迁移。
- 后续路径修正：`app_local_data_dir()` 自动添加应用标识，用户要求去掉这一层，因此改用 `local_data_dir()`
  。新位置不存在数据库时在线备份导入旧位置，已有数据库不自动覆盖，旧文件保留。
- 本机两处已有数据库，用户选择沿用最新配置：已先备份目标版本 1 数据库至
  `C:/Users/minyu/AppData/Local/EasyVideoDownload/app.backup-before-path-change-20261003-144610.db`，再在线备份版本 2
  到新位置；5 项设置和 4 个程序配置逐行比较一致，quick_check 通过。该备份为正式恢复文件，保留。
- 路径修正验证：Rust 37 项通过、1 项既有辅助测试忽略；前端 14 项通过；构建、格式和 Clippy 通过。新增验证覆盖 WAL
  数据迁移、已有目标库不被自动覆盖及迁移失败不留下空数据库。
- 后续时间修正：用户指定标准北京时间且不带 `+00:00`。保留历史 SQL，新增版本 3 事务迁移，将旧 UTC 时间转为固定 UTC+8；Rust 写入与
  SQL 默认值统一为无后缀文本。整数秒和旧 UTC JSON 在导入时转换，已是北京时间的 JSON 不再偏移。
- 时间修正验证：先运行 UTC 转换回归测试并确认失败，再实现迁移。Rust 41 项通过、1 项既有辅助测试忽略，覆盖跨日转换、重复启动不重复加
  8 小时、无效日期及溢出时整体回滚、旧 JSON 兼容和 Rust/SQL 新写入的北京时间。
- 本机应用已自动迁移实际数据库至版本 3；只读核对 5 项设置和 4 个程序的时间均无后缀，`2026-10-03 06:33:51+00:00` 已转换为
  `2026-10-03 14:33:51`，quick_check 通过。
- 时间修正最终检查：前端 14 项通过，pnpm build、cargo fmt --check、cargo clippy --offline --all-targets -- -D warnings
  通过；未暂存或提交。
