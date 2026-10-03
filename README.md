---
updated: "2026-10-03"
tags:
  - EasyVideoDownload
  - Tauri
  - yt-dlp
  - 项目规划
---

## 项目定位

项目名称：**EasyVideoDownload**  
项目目录：`C:\Users\minyu\Projects\EasyVideoDownload`  
目标平台：Windows、macOS、Linux 桌面端。  
框架：使用 **Tauri 2 + Rust**，前端采用 **Vue 3 + TypeScript + Vite + Element Plus**。

希望做一个简洁、好用、界面精致的视频下载器，让用户轻松下载视频。核心价值是减少下载所需的操作，同时取得当前账号能够访问的高画质资源。界面围绕用户的下载任务设计，避免堆砌 yt-dlp 的全部参数。

本文保留项目能力基线与早期讨论背景。界面与交互已有 v0.6 定稿，随后确认跨平台目标、两种工具来源与前端技术；最新范围、技术清单及待办见 [项目讨论稿](docs/project-discussion.md)。当前只有文档与模拟原型，尚未搭建正式应用。

## 已确定的需求与取舍

- 从零开发，使用 Tauri；已有客户端的界面与任务流程没有满足需求。
- 主要关注抖音、Bilibili、YouTube，同时利用 yt-dlp 扩展其他平台的下载支持。
- 保留必要的下载步骤，重视布局、交互反馈与使用体验。
- 希望能够方便地下载最高可用画质，尤其是登录后的 B 站大会员资源。
- 复用电脑上已经安装的工具，仅提供系统 PATH 检测和手动选择两种来源；应用不负责下载、安装或更新工具。
- 下载失败应给出可以理解的原因与下一步操作，下载完成应明确提示实际保存位置。
- 原先考虑过 f2 和 VideoCaptioner；当前重点已经转为视频下载器。用户更认可 yt-dlp，首版以它为下载核心。

## 首版主流程

1. **粘贴链接**：输入视频网址；抖音分享文本中的链接可作为需要支持的输入形式。
2. **解析视频**：显示标题及可用画质，让用户知道即将下载什么。解析期间有明确状态，失败或超时可重试。
3. **确认下载**：默认选择最高可用画质，提供简洁的画质选择与保存目录选择，并记住上次使用的目录。
4. **执行下载**：显示下载、合并等实际阶段，以及能够取得的进度、速度；允许取消和失败后重试。
5. **完成提示**：显示最终文件与保存路径，提供打开文件、打开文件夹的入口。

手动选择平台后执行以上流程，一次下载一个视频。Cookie 从当前平台工具栏展开导入；工具路径放在设置页。默认容器、文件命名和历史保存策略仍需确定，详细交互以项目讨论稿与原型为准。

## 画质选择与已验证的命令

用户已经使用 B 站大会员 Cookie，通过以下命令成功下载了约 **1.7 GB** 的视频。这是后续开发应保留的能力基线：

```powershell
yt-dlp --cookies "C:\Users\minyu\Downloads\www.bilibili.com_cookies.txt" --no-playlist -f "bv*+ba/b" -S "res,br,fps" --format-sort-force --merge-output-format mkv -P "C:\Users\minyu\Videos\BiliBili\yt-dlp" "https://www.bilibili.com/bangumi/play/ep785561"
```

当前命令优先分辨率，再按码率、帧率排序；选择视频和音频，并交给 FFmpeg 合并。MKV 是这次成功验证使用的容器，应用的默认容器仍需讨论。

开发时需要注意：

- “最高画质”受平台实际提供的格式、Cookie 有效性、账号权限等条件限制。
- 同分辨率下可以优先高码率，但跨编码比较时不能仅靠码率判断画质。
- 高画质资源可能是独立视频流和音频流，需要下载后合并。
- 显示文件大小或码率时，应区分真实数据和估算值，不能把估算当作实测。
- 解析得到的高画质格式应真正用于下载，不能只在界面中显示标签而最终下载较低规格。
- 首版建议默认下载单个视频，合集或播放列表下载作为后续范围讨论。

## Cookie 与后续登录方案

首版采用 Netscape 格式 Cookie 文件导入与文本粘贴，按平台独立保存到应用数据目录；通过 Tauri 的跨平台路径 API 获取目录，再把文件路径交给 yt-dlp。

早期曾希望应用内打开平台页面，通过扫码等登录方式取得 Cookie。统一扫码登录已暂缓；若后续实现，需要逐平台验证 Tauri WebView 登录、Cookie 读取及登录状态复用。

Cookie 属于账号凭据，应保存在本机适当位置，日志与错误信息应脱敏，不能写入源码或示例文档。本文只记录 Cookie 文件路径，没有记录 Cookie 内容。

## 技术方向与已知问题

由 Tauri 的 Rust 后端负责调用下载工具、管理子进程与任务状态，Vue 3 + TypeScript 前端负责输入、选择和反馈，Vite 负责前端构建，Element Plus 提供基础交互组件。配套技术建议清单见项目讨论稿，尚未安装依赖。

- 以 yt-dlp 为核心，FFmpeg 负责合并，FFprobe 可用于验证最终视频规格。
- 调用外部工具时使用结构化参数，避免把用户输入拼接为 shell 命令。
- 解析和下载放在后台运行，提供取消、超时与错误处理，保持界面可操作。
- 区分解析失败、登录失效、下载失败、合并失败、保存路径不可写等情况。
- 工具检测应说明使用的是哪一个可执行文件；缺失时提供手动配置与官方安装说明入口。
- 用户反馈当前 yt-dlp 命令下载的抖音画质仍不足；已完成 Videdown 源码调查，下一步比较同一视频的候选资源与实际文件，再决定专用解析适配方案。
- Tauri 的 Windows Cookie 读取存在同步调用死锁的文档提示；实现前核对所用版本的官方文档，在异步命令或合适线程中处理。

Videdown 的源码调查与待验证方案见[项目讨论稿](docs/project-discussion.md#videdown-抖音源码调查2026-10-03)。本次仅检查源码，没有执行抖音解析或下载，尚未证实它能获得更高画质。

用户试用过的部分客户端出现启动白屏、按钮错位、解析失败、卡死、缺少保存反馈等问题。新项目应把这些问题作为体验验证重点。

## 本机环境

以下可执行文件在 2026-10-02 已通过命令查找确认存在：

| 工具 | 当前路径 |
| --- | --- |
| yt-dlp | `C:\Users\minyu\scoop\apps\python312\current\Scripts\yt-dlp.exe` |
| FFmpeg | `C:\Users\minyu\scoop\shims\ffmpeg.exe` |
| FFprobe | `C:\Users\minyu\scoop\shims\ffprobe.exe` |
| Node.js | `C:\Program Files\nodejs\node.exe` |
| pnpm | `C:\Users\minyu\scoop\shims\pnpm.exe` |
| VideoCaptioner | `C:\Users\minyu\scoop\apps\python312\current\Scripts\videocaptioner.exe` |

本机 yt-dlp 是 Python 3.12 环境下的命令行程序，此前随 VideoCaptioner 依赖安装。还有客户端管理的另一份 yt-dlp；后续应明确实际使用的路径，避免版本混淆。

当前 `cargo`、`rustc` 未在 PATH 中找到。正式开发前还需检查 Rust、Windows C++ 构建工具和 WebView2，按需准备 Tauri 开发环境。

用户已验证 VideoCaptioner 可以将本地视频转为文字。转写功能可以后续讨论，目前不把转写、翻译、字幕编辑加入首版下载流程。

## 下一步工作建议

1. 复核项目讨论稿中的技术清单，确定存储、状态管理与任务实现细节。
2. 检查开发环境，并通过抖音、B 站、YouTube 的实际样例验证解析、格式选择和下载能力。
3. 完成界面与技术设计，形成用户可审阅的实施方案，再搭建 Tauri 项目。
4. 先跑通链接解析、最高可用画质下载、音视频合并、保存完成提示这一条完整流程。
5. 验证失败、取消、重试、重复文件与不可写目录等场景，并检查窗口缩放和启动显示。

验收应以实际文件为依据：视频可以播放且有声音，规格符合选中的资源，保存位置明确，错误发生后界面仍能继续使用。各平台是否可用、高画质是否保留，需要记录真实测试结果。

截至交接时，项目尚未搭建，开发依赖尚未安装，也未执行 Git 初始化。Git 初始化、提交、推送、分支等改变状态的操作，需要先说明具体命令与影响，并取得用户明确确认。

## 参考源码与官方文档

- [yt-dlp 源码与参数说明](https://github.com/yt-dlp/yt-dlp)
- [VideoCaptioner 源码](https://github.com/WEIFENG2333/VideoCaptioner)
- [Videdown 源码：抖音解析思路参考](https://github.com/cshuangyy/videdown)
- [imsyy 的 yt-dlp-gui：Tauri 调用方式参考](https://github.com/imsyy/yt-dlp-gui)
- [Tauri 开发环境要求](https://v2.tauri.app/start/prerequisites/)
- [Tauri 外部程序集成](https://v2.tauri.app/develop/sidecar/)
- [Tauri WebView Cookie API](https://docs.rs/tauri/latest/tauri/webview/struct.WebviewWindow.html#method.cookies_for_url)
