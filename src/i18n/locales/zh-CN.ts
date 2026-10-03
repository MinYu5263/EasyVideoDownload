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
    },
    languages: {
        simplifiedChinese: "简体中文",
        english: "English",
    },
};

export type MessageSchema = typeof zhCN;
export default zhCN;
