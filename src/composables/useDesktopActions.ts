import {invoke, isTauri} from "@tauri-apps/api/core";

export const toolWebsites = {
    ytdlp: "https://github.com/yt-dlp/yt-dlp",
    ffmpeg: "https://ffmpeg.org/",
    deno: "https://deno.com/",
} as const;

interface DesktopBridge {
    desktop: boolean;
    invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
}

export function createDesktopActions(bridge: DesktopBridge) {
    function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
        if (!bridge.desktop) return Promise.reject({code: "desktopOnly"});
        return bridge.invoke<T>(command, args);
    }

    return {
        desktop: bridge.desktop,
        readClipboard: () => call<string>("plugin:clipboard-manager|read_text"),
        writeClipboard: (text: string) => call<void>("plugin:clipboard-manager|write_text", {text}),
        importCookie: () => call<string | null>("import_cookie_file"),
        chooseDirectory: () => call<string | null>("select_download_directory"),
        openToolWebsite: (tool: keyof typeof toolWebsites) => call<void>("plugin:opener|open_url", {url: toolWebsites[tool]}),
    };
}

export function useDesktopActions() {
    return createDesktopActions({desktop: isTauri(), invoke});
}
