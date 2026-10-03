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
        tools: {
            title: "Required tools",
            description: "Use existing programs from your system PATH or choose them manually.",
            detectAll: "Check all",
            pending: "Not checked",
            source: "Tool source",
            sourceLabel: "{tool} source",
            systemPath: "System PATH",
            manual: "Choose manually",
            pathHelp: "Find existing programs on your system PATH without reinstalling them.",
            detectPath: "Check PATH",
            directory: "Directory containing FFmpeg and FFprobe",
            programPath: "Program path",
            pathLabel: "{tool} program path",
            directoryLabel: "{tool} directory",
            directoryPlaceholder: "Enter the directory containing FFmpeg and FFprobe",
            pathPlaceholder: "Enter the full path to an existing program",
            chooseDirectory: "Choose folder",
            chooseProgram: "Choose program",
            manualHelp: "No PATH changes needed. The program will be used after a successful check.",
            detectAndUse: "Check and use",
            version: "Version",
            resolvedPath: "Resolved path",
            notDetected: "Not checked",
            runtimeNote: "YouTube parsing components will be checked together with yt-dlp.",
            previewNotice: "Tool checks are not available yet. No programs will run or tool settings be saved.",
            browseNotice: "File and folder browsing will be added later. You can enter a path manually for now.",
            ytdlp: {
                name: "yt-dlp",
                kind: "Core tool",
                description: "Video link parsing and downloading",
            },
            ffmpeg: {
                name: "FFmpeg + FFprobe",
                kind: "Merge tools",
                description: "Merge video and audio, and inspect the final file",
            },
            runtime: {
                name: "JavaScript runtime",
                kind: "YouTube support",
                description: "A runtime for YouTube parsing. Use an existing Node.js or Deno installation.",
            },
        },
        about: {
            title: "About",
            tagline: "Save simply, one video at a time.",
            version: "Current version",
            platforms: "Target platforms",
            links: "Official tool websites",
        },
    },
    languages: {
        simplifiedChinese: "简体中文",
        english: "English",
    },
} satisfies MessageSchema;

export default en;
