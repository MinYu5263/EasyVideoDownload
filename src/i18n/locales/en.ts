import type {MessageSchema} from "./zh-CN";

const en = {
    accessibility: {
        skipToContent: "Skip to content",
        sidebar: "Application sidebar",
        mainNavigation: "Main navigation",
        pageNavigation: "Page navigation",
    },
    navigation: {
        download: "Video download",
        history: "Download history",
        settings: "Settings",
        recordCount:
            "No downloads in history | {count} download in history | {count} downloads in history",
    },
    pages: {
        download: {
            emptyTitle: "Waiting for a video link",
            emptyDescription:
                "Choose a platform and paste a link to see available download options.",
        },
        history: {
            emptyTitle: "No completed downloads yet",
            emptyDescription: "Find your video files here after a download finishes.",
        },
        settings: {
            emptyTitle: "Make downloading easier",
            emptyDescription: "Language, required tools, and application information.",
        },
    },
    settings: {
        application: "Application settings",
        language: "Interface language",
        languageDescription:
            "Choose your preferred language. Changes take effect immediately.",
    },
    languages: {
        simplifiedChinese: "简体中文",
        english: "English",
    },
} satisfies MessageSchema;

export default en;
