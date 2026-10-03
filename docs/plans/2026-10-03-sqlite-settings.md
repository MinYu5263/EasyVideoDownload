# SQLite 设置持久化实施计划

> 执行方式：在当前工作区继续实现；遵守用户 Git 规则，不创建分支或 worktree，不提交。

**目标**：将界面语言和必备工具配置统一保存到 SQLite，增加四项可保存的设置界面。

> 此文记录首次接入方案。后续已调整为两张业务表、键值设置与北京时间文本，当前结构见 [本地数据库](../database.md)
> ，升级记录见 [结构第二版](2026-10-03-sqlite-schema-v2.md)。

**设计依据**：本次对话已确认 Rust + rusqlite、EasyVideoDownload/app.db、三个设置表；用户要求先完成设置页面及持久化，实际主题、通知和关闭行为后续实现。

**架构**：Rust 数据库模块管理连接、结构升级、旧 JSON 导入和事务。Vue 使用设置控制器调用类型明确的 Tauri
命令；保存成功后应用新值，失败保留旧值。数据库操作在线程池中执行。

**约束**：

- 数据库位于 app_local_data_dir ()/EasyVideoDownload/app.db，使用用户指定的 Tauri 跨平台路径 API（应用本地数据目录包含
  bundle identifier）。
- 保留现有 WebView 数据目录，避免移动目录后丢失旧 localStorage 语言；首次读取设置时导入旧语言，已有数据库值优先。
- 表名为 app_settings、required_tools、required_tool_programs；程序配置必须整组提交。
- 语言保留 zh-CN / en；theme = system / light / dark，默认 system。
- notify_on_completion 与 notify_on_failure 默认 true；close_action = ask / tray / exit，默认 ask。
- 四项新设置仅保存偏好，页面注明后续接入；语言仍立即生效。
- 旧 required-tools.json、program-dependencies.json、tools.json 保留，SQLite 导入成功后停止写 JSON。
- 损坏旧文件或数据库不得被静默覆盖；未知新版数据库不得降级写入。
- 不新增主题切换、通知权限、托盘、关闭事件等运行逻辑，不扩大下载功能范围。

## 实施步骤

- [x] 数据库：增加 rusqlite bundled 依赖、第一版 DDL、连接管理、事务和一次性旧配置导入；测试重启恢复、失败回退、损坏文件及新版数据库保护。
- [x] 工具：保留现有检测逻辑，将检测成功的整组配置写入 SQLite；失败继续使用旧配置，启动不运行检测。
- [x] 应用设置：增加读取和保存命令，以及 Vue 控制器；启动先恢复语言，保存失败不改变已保存值，快速更新不丢失。
- [x] 页面：扩展现有应用设置卡片，提供主题/关闭行为下拉框、完成/失败通知开关、保存状态和错误提示，中英文一致。
- [x] 待办：记录主题、通知和关闭行为的真实功能接入。
- [x] 验证：运行 Rust/前端测试、前端构建、格式及编译检查，检查页面布局，完成一次独立代码审查。

## 审查重点

1. JSON 和旧语言的迁移不会覆盖已保存的 SQLite 设置。
2. FFmpeg/FFprobe 保存失败不会留下不完整的新配置。
3. 并发保存工具和应用设置不会互相覆盖。
4. 加载失败不会把默认值误保存为用户设置；保存失败可重试。
5. 浏览器仅预览，页面如实标明本地持久化需要桌面应用。

## 进度记录

- 路径选择：使用 app_local_data_dir ()/EasyVideoDownload/app.db，保留默认 WebView 路径，以延续旧语言的一次性导入；不覆盖全局
  localData 目录。
- 验证：前端 11 项测试通过，Rust 24 项测试通过（1 项既有辅助测试忽略）；pnpm build、cargo fmt 检查和 Clippy 通过。
- 浏览器预览已确认中英文、主题和关闭选项、通知开关及布局；真实 SQLite 的关闭后重新打开、迁移及事务回滚通过后端测试验证，未进行原生桌面端
  UI 重启验证。
- 独立审查无 Critical / Important 问题；已移除无效的初始化失败重载入口，加载错误提示用户修复数据文件并重启。
