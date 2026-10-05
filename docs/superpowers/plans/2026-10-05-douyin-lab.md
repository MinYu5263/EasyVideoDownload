# 抖音实验页 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans.
> 用户已批准在当前会话直接执行。实现与自动验证已完成，实验页交互的桌面手动验收未完成；结果记录见文末。

**Goal:** 在独立侧栏页面完成抖音 Rust HTTP 解析、格式选择、直链下载、取消及实际媒体信息核对。

**Architecture:** 独立前端控制器与 Rust 实验任务管理器通过原生命令和事件通信。复用
CookieStore、平台代理、工具配置与原生目录选择接口，保持正式下载与历史流程独立。实验导航仅在当前会话保存。

**Tech Stack:** Vue 3、TypeScript、Element Plus、Tauri 2、Rust、reqwest 0.13.5、Tokio、ffprobe；在已有 reqwest 上启用
cookies，在已有 Tokio 上启用 fs。

**Spec:** `docs/superpowers/specs/2026-10-05-douyin-lab-design.md`

## Global Constraints

- 解析使用自定义 Rust HTTP 实现，复用用户已导入的 Cookie。
- 同一时刻只允许一个实验解析或下载操作；切换页面继续运行，应用重启清空状态。
- 不接入正式队列、历史记录或数据库迁移；不引入浏览器自动化、批量下载和断点续传。
- 默认目录为 `Videos/EasyVideoDownload/douyin-lab`；目录选择使用 Tauri，文件不覆盖。
- Cookie 与签名媒体 URL 不进入前端或日志；未知媒体字段显示未知。
- ffprobe 缺失时允许解析，但禁用实验下载；完成状态需经过实际文件核对。
- 保留用户工作区修改；不执行 Git 状态变更、提交、推送或远程操作。
- 独立前端测试位于被忽略的 `tests/`，Rust 测试随对应模块保存。记录并清理本任务创建的临时内容。

## Review Focus

1. Cookie 的 host-only、子域、路径、HTTPS 和过期规则必须阻止跨域发送。
2. 同分辨率的 H.264/H.265、多种码率和镜像必须保留正确身份，不能混为一个格式。
3. 取消后晚到的结果、事件或下载完成不得覆盖新的状态或提交正式文件。
4. 重名文件、签名过期及镜像重试不得覆盖已有文件或静默降级。
5. 真正退出必须等待实验文件清理和 ffprobe 停止；不访问实验页时不启动实验请求。

## 文件职责与接口约定

- `src-tauri/src/douyin/mod.rs`：公开错误、媒体/格式/解析结果类型。前端结构使用 camelCase；内部候选包含镜像 URL，公开格式不包含
  URL。
- `src-tauri/src/douyin/cookies.rs`：Netscape Cookie 到 reqwest Jar，拒绝无有效抖音 Cookie 的解析操作。
- `src-tauri/src/douyin/parse.rs`：链接验证、重定向、详情请求、格式整理及稳定 ID。
- `src-tauri/src/douyin/download.rs`：流式文件接收、镜像/刷新、ffprobe、无覆盖发布。
- `src-tauri/src/douyin/lab.rs`：会话缓存、单任务管理、取消信号、退出协调、Tauri 命令及事件。
- 各模块 `#[cfg(test)]` 测试或相邻 `tests.rs`：纯逻辑、网络本地服务器及文件系统测试。
- `src/composables/douyinLabTypes.ts`：实验专用 DTO；`src/composables/useDouyinLab.ts`：独立状态连接和操作。
- `src/components/DouyinLabPage.vue`：真实输入、格式列表、下载进度与媒体结果。
- 修改 `src/navigation.ts`、`src/App.vue`、`src/composables/useUiPreferences.ts` 和两个 locale：导航接入及持久化类型隔离。
- 修改 `src-tauri/src/lib.rs`、`src-tauri/src/app_preferences.rs`、Cargo.toml/Cargo.lock：模块注册、退出协调及所需依赖特性。

公开 `LabSnapshot` 包含
sessionId、revision、taskId、phase、parsed、receivedBytes、totalBytes、outputPath、observed、error，以及不含凭据的配置信息。phase 为
idle/parsing/ready/downloading/verifying/cancelling/completed/cancelled/failed。`LabError` 只携带固定 code 和经清理的
detail，不直接使用含 URL 的 reqwest 错误文本。

原生命令：`douyin_lab_get_state()`、`douyin_lab_parse(url: String)`、
`douyin_lab_download(result_id: String, format_id: String, directory: String)`、`douyin_lab_cancel(task_id: String)`；均返回
`Result<LabSnapshot, LabError>`。启动命令接收任务后返回快照，后台工作通过 `douyin-lab-changed` 推送快照。状态查询提供当前默认目录、Cookie
是否可用及 ffprobe 是否可用，不返回 Cookie 内容。

内部 `LabManager` 提供 `begin_exit() -> Result<(), String>`、`is_active() -> Result<bool, String>`、`abort_exit()`
。解析缓存持有本次 reqwest Client 和凭据/代理快照，下载命令只使用缓存里的候选，不能接受前端媒体 URL。

## Task 1: HTTP 解析与格式身份

**Files:** 新增 douyin/mod.rs、cookies.rs、parse.rs 及对应 Rust 测试；修改 Cargo.toml/Cargo.lock，lib.rs 仅注册模块以编译测试。

**Interfaces:** `cookie_jar(contents: &str) -> Result<Arc<reqwest::cookie::Jar>, LabError>`；
`normalize_link(input: &str) -> Result<url::Url, LabError>`；
`parse_detail(value: &serde_json::Value) -> Result<ParsedResult, LabError>`；HTTP 解析函数返回同时含公开媒体信息与私有候选的
ParsedResult。

- [ ] 写失败测试：非抖音 URL 拒绝；Cookie 不发送给无关域名、host-only 不扩展、过期/路径/HTTPS 生效；格式读取地址中的
  1920×1080/data_size 与 FPS；同尺寸不同编码和码率保留，镜像合并且 ID 稳定。
- [ ] 执行 `cargo test --manifest-path src-tauri/Cargo.toml douyin::`，确认缺失行为导致失败。
- [ ] 实现 Netscape Cookie 导入和有边界的短链重定向；构建同一解析会话的 Cookie/代理 Client；请求详情携带
  aweme_id、aid=6383、channel=channel_pc_web、detail_list=1。为空、状态异常、不可支持作品时返回固定错误。
- [ ] 实现地址字段优先的格式整理；gear_name/url_key 仅作补充，未知尺寸不猜值。限制元数据响应大小与请求超时；请求失败不暴露
  Cookie 或完整签名地址。
- [ ] 重跑测试直到通过。用本地 HTTP 服务器验证空响应、重定向和请求参数；真实平台响应仅用于桌面验收，不作为必须联网的单元测试。

## Task 2: 可取消下载与媒体核对

**Files:** 新增 douyin/download.rs 和测试。

**Interfaces:** 下载输入为私有解析缓存、候选、目录、取消信号和字节进度回调；输出 `DownloadedMedia { output_path, observed }`
，observed 记录实测 width/height/codec/duration/bitrate/fileSize。

- [ ] 写失败测试：本地服务器的真实字节进度、镜像失败切换、Content-Length 不完整、取消删除临时文件、已有同名文件保留、刷新找不到原格式报错；FFprobe
  JSON 正确读取与尺寸不匹配拒绝完成。
- [ ] 执行对应 Rust 测试，确认失败源自未实现行为。
- [ ] 实现任务唯一临时文件、流式写入、取消与超时、镜像重试；HTTP 401/403/410 导致签名刷新最多一次，按原视频/编码/尺寸/质量特征匹配候选，无法匹配时不换格式。
- [ ] 复用 RequiredToolManager::settings_and_usage 取得工具快照与使用租约，使用配置中的 ffprobe；子进程设
  kill-on-drop、超时与取消处理，避免取消时遗留运行进程。核对真实媒体后使用 tempfile 无覆盖发布，重名增加后缀。
- [ ] 重跑测试；在临时目录执行真实文件/ffprobe 接口验证。只清理本任务测试产生的明确路径。

## Task 3: 实验任务与原生生命周期

**Files:** 新增 douyin/lab.rs 和测试；修改 lib.rs、app_preferences.rs。

**Interfaces:** 实现上述四个原生命令、独立事件和 LabManager 退出接口；从 CookieStore::load (Douyin)、proxy::platform_proxy
和现有工具管理器读取配置。get_state 计算默认目录但不创建文件夹或启动网络请求。

- [ ] 写失败测试：并发启动只有一个成功；失效 resultId/formatId 拒绝；只有当前 taskId
  可取消；版本递增；取消期间晚到结果不能完成；退出拒绝新任务并等待清理，abort_exit 恢复可启动状态。
- [ ] 执行对应测试，确认失败后实现单任务状态机与缓存生命周期。
- [ ] 注册原生命令和管理器；解析/下载在后台运行，失败和取消均释放活动标记。后台结束必须晚于临时文件和子进程清理；终态快照保留便于回到页面查看。
- [ ] 在 request_exit 与 settle_on_native_exit 的既有协调循环中加入实验 begin_exit/is_active；退出失败时加入
  abort_exit。不改变主下载管理器和工具配置任务的运行逻辑。
- [ ] 重跑实验状态测试及退出相关现有 Rust 测试，确认正式流程无回归。

## Task 4: 实验页面与导航隔离

**Files:** 新增 douyinLabTypes.ts、useDouyinLab.ts、DouyinLabPage.vue 和本地 tests/douyin-lab.test.mjs；修改
navigation.ts、App.vue、useUiPreferences.ts、zh-CN.ts、en.ts。

**Interfaces:** `createDouyinLab(bridge)` 返回 state、selectedFormatId、parse、download、cancel、connect、dispose；bridge
使用真实原生 invoke/listen 的适配接口。测试事件适配仅验证状态逻辑，不作为桌面操作验证证据。

- [ ] 写失败测试：旧 session/revision 事件不能回退状态；断开再连接读取原生快照；未完成连接时禁用操作；目录选择取消保留原目录；实验导航不写入数据库偏好。
- [ ] 执行 `node --test tests/douyin-lab.test.mjs` 确认行为失败。
- [ ] 实现先监听后读状态的连接流程和注销处理；原生调用失败时展示错误，不创建假进度或假结果。页面只读取配置可用状态，不调用会把
  Cookie 内容带到前端的 get_cookie_contents。
- [ ] 新增实验导航，并将持久化页面类型限制为 download/history/settings；App.vue 使用会话内实验导航覆盖值，首次访问才挂载，之后
  v-show 保持后台状态。普通页面切换仍沿用原持久化行为。
- [ ] 实现表格和格式选择：真实分辨率优先，同尺寸 H.264 优先；未知帧率不排除；字段未知明确展示；用现有 useDesktopActions
  的原生剪贴板与目录选择接口。页面显示实际进度及 ffprobe 核对结果，中英文文案完整。
- [ ] 重跑前端测试并执行 `npm run build`，确认类型与构建通过。

## Task 5: 桌面验收与最终检查

**Files:** 更新规格/计划的完成情况；需要修正时只改对应实验模块或最小集成位置。

- [ ] 执行 `cargo test --manifest-path src-tauri/Cargo.toml`、`npm test`、`npm run build`；如出现已有失败，记录测试名称和原因，不省略。
- [ ] 启动桌面开发版本，通过真实 Tauri
  调用验证配置读取、短链解析、目录选择、下载和取消；样本为 https://v.douyin.com/V4Jkr52cl90/，核对 1080p H.264 与 720p H.265
  的候选和实际下载尺寸。所需输出路径在运行前记录；验收文件只在确认用途后清理。
- [ ] 验证未访问实验页不发实验请求；切换页面下载继续；真正退出停止实验任务且清理临时文件；重名不覆盖；正式主页面解析、下载、历史和设置仍可正常使用。
- [ ] 检查前端真实调用路径、插件注册、capability、取消/失败和键盘访问；不添加浏览器原生功能替代。无法实际验证的系统行为逐项标为未验证。
- [ ] 使用 `git diff --check`、`git diff --stat` 和定向 diff 检查范围，记录完成项与限制。所有 Git 操作只读，不自行提交。

## 执行结果（2026-10-05）

- 用户批准后在 feature/douyin-lab 实现；未执行 Git 提交或推送。
- Rust 完整测试：249 通过、16 默认忽略；抖音模块：15 通过、1 默认忽略。前端完整测试：273 通过；类型检查与 Vite 构建通过。
- 额外执行需联网的原生 Rust 样本测试：24 个格式，实际下载并核对 1920×1080 H.264，47,219,203 字节，2,220,338 bps，170.133333
  秒；任务拥有的临时输出已清理。
- 额外执行真实 Tauri 窗口/托盘/退出回归测试，通过。实验页目录选择、剪贴板和布局交互仍待桌面手动验收，不以浏览器预览代替。
- 独立审查指出签名刷新对未知质量的兜底比较不可靠；两项回归先失败后通过，最终只匹配原稳定格式 ID。
- 实验页组件按需加载，导航状态仅保存在本次会话；默认实验输出目录独立，正式下载与历史流程保持原实现。
- 为使既有验证可靠，修正本地历史状态测试对 CSS class 顺序的依赖；既有代理测试释放 SQLite 后再清理临时目录，并给单独子进程启动保留
  15 秒，而普通进程内探测仍为 3 秒。均未改生产代理逻辑。
- 偏离原文件清单：新增 usePageNavigation.ts 以直接测试导航隔离。使用已有用户分支，不创建额外 worktree。
- Vite 保留大于 500 kB 的主包提示；实验页独立加载以减少其对首屏的影响。
