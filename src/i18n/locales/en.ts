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
        requiredTools: {
            title: "Required tools",
            pending: "Not checked",
            checkAll: "Check all",
            check: "Check",
            checkLabel: "Check {program}",
            officialWebsite: "Visit the {program} website",
            websiteOpenFailed: "Could not open the website. Visit it in your browser: {url}",
            installed: "Installed",
            missing: "Not found",
            unavailable: "Unavailable",
            checking: "Checking",
            keepPrevious: "The saved configuration below is still active.",
            draftNotice: "The current input has not been applied. The saved program is still active.",
            errorDetails: "View error details",
            desktopOnly: "Open the desktop app to check tools and choose local paths. The browser shows a preview only.",
            errors: {
                notFound: "{program} was not found. Check PATH or choose an existing program.",
                invalidPath: "Enter an absolute program path or tool directory. On Windows, choose an .exe file.",
                invalidVersion: "Could not recognize the {program} version. Check that you selected the correct program.",
                invalidProgram: "The selected program could not be identified as {program}. Choose the correct program.",
                denoTooOld: "Deno 2.3.0 or newer is required.",
                spawnFailed: "Could not start {program}. Check its path, permissions, and dependencies.",
                exitFailed: "{program} exited with an error. See the details below.",
                timeout: "The {program} check exceeded 10 seconds and was terminated.",
                outputTooLarge: "{program} output exceeded the 64 KiB limit.",
                readFailed: "Could not read the {program} check result.",
                saveFailed: "Could not save the configuration. The new configuration was not applied.",
                loadFailed: "Could not read tool settings. Tool operations are paused to preserve the file. Check the file and restart the app.",
                dialogFailed: "Could not open or read the local path picker.",
                busy: "This tool is being checked. Wait for it to finish.",
                bridgeFailed: "Could not connect to the desktop service. Try again or restart the app.",
            },
            sourceLabel: "{program} source",
            systemPath: "System PATH",
            manual: "Choose manually",
            directory: "Directory containing FFmpeg and FFprobe",
            programPath: "Program path",
            pathLabel: "{program} program path",
            directoryLabel: "{program} directory",
            directoryPlaceholder: "Enter the directory containing FFmpeg and FFprobe",
            pathPlaceholder: "Enter the full path to an existing program",
            chooseDirectory: "Choose folder",
            chooseProgram: "Choose program",
            version: "Version",
            ytdlp: {
                name: "yt-dlp",
                description: "Video link parsing and downloading",
            },
            ffmpeg: {
                name: "FFmpeg + FFprobe",
                description: "Merge video and audio, and inspect the final file",
            },
            deno: {
                name: "Deno",
                description: "JavaScript runtime for YouTube parsing. Requires version 2.3.0 or newer.",
            },
        },
        about: {
            title: "About",
            version: "Current version",
        },
    },
    languages: {
        simplifiedChinese: "简体中文",
        english: "English",
    },
} satisfies MessageSchema;

export default en;
