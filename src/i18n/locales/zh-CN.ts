const zhCN = {
    accessibility: {
        skipToContent: "跳到页面内容",
        sidebar: "应用侧边栏",
        mainNavigation: "主导航",
        pageNavigation: "页面导航",
    },
    navigation: {
        download: "视频下载",
        history: "下载记录",
        settings: "设置",
        recordCount: "{count} 条下载记录",
    },
    pages: {
        download: {
            emptyTitle: "等待视频链接",
            emptyDescription: "选择平台、粘贴链接并解析，查看可用的下载选项。",
        },
        history: {
            emptyTitle: "还没有完成的下载",
            emptyDescription: "下载完成后，可以在这里找到视频文件。",
        },
        settings: {
            emptyTitle: "让下载更顺手",
            emptyDescription: "界面语言、必备工具与应用信息。",
        },
    },
    settings: {
        application: "应用设置",
        language: "界面语言",
        languageDescription: "选择使用时显示的语言，修改后立即生效。",
        tools: {
            title: "必备工具",
            description: "复用电脑上已有的程序，通过系统 PATH 查找或手动选择。",
            detectAll: "检测全部",
            pending: "待检测",
            source: "使用方式",
            sourceLabel: "{tool} 使用方式",
            systemPath: "系统 PATH",
            manual: "手动选择",
            pathHelp: "查找系统 PATH 中已有的程序，无需重复安装。",
            detectPath: "检测 PATH",
            directory: "FFmpeg 与 FFprobe 所在目录",
            programPath: "程序路径",
            pathLabel: "{tool} 程序路径",
            directoryLabel: "{tool} 所在目录",
            directoryPlaceholder: "输入 FFmpeg 与 FFprobe 所在目录",
            pathPlaceholder: "输入已有程序的完整路径",
            chooseDirectory: "选择目录",
            chooseProgram: "选择程序",
            manualHelp: "无需配置环境变量，检测通过后使用所选程序。",
            detectAndUse: "检测并使用",
            version: "版本",
            resolvedPath: "实际使用路径",
            notDetected: "未检测",
            runtimeNote: "YouTube 解析组件将与 yt-dlp 一并检测。",
            previewNotice: "工具检测尚未接入，当前不会运行程序或保存工具配置。",
            browseNotice: "程序与目录选择功能将在后续接入，当前可手动输入路径。",
            ytdlp: {
                name: "yt-dlp",
                kind: "核心工具",
                description: "链接解析与视频下载",
            },
            ffmpeg: {
                name: "FFmpeg + FFprobe",
                kind: "合并工具",
                description: "合并音视频，并检查最终文件规格",
            },
            runtime: {
                name: "JavaScript 运行时",
                kind: "YouTube 支持",
                description: "为 YouTube 解析提供运行环境，可复用 Node.js 或 Deno。",
            },
        },
        about: {
            title: "关于",
            tagline: "简单保存，一个视频一次。",
            version: "当前版本",
            platforms: "目标平台",
            links: "工具官方网站",
        },
    },
    languages: {
        simplifiedChinese: "简体中文",
        english: "English",
    },
};

export type MessageSchema = typeof zhCN;
export default zhCN;
