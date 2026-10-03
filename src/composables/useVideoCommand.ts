import {ref} from "vue";
import {validateVideoLink, type VideoPlatform} from "./videoPlatforms.ts";

export interface DownloadCommandOptions {
    directory: string;
    formatId: string;
}

interface VideoCommand {
    text: string;
    shell: string
}

interface CommandError {
    code: string;
    detail?: string
}

interface CommandBridge {
    desktop: boolean;
    invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
    beforeRead: (platform: VideoPlatform) => Promise<boolean>;
    writeClipboard: (text: string) => Promise<void>;
}

export function createVideoCommand(bridge: CommandBridge) {
    const open = ref(false);
    const command = ref<VideoCommand | null>(null);
    const loading = ref(false);
    const copying = ref(false);
    const error = ref<CommandError | null>(null);
    let version = 0;
    let disposed = false;

    function close() {
        version += 1;
        open.value = loading.value = copying.value = false;
        command.value = error.value = null;
    }

    function dispose() {
        disposed = true;
        close();
    }

    async function show(platform: VideoPlatform, input: string, options?: DownloadCommandOptions) {
        if (disposed) return;
        close();
        const current = version;
        open.value = loading.value = true;
        try {
            if (!bridge.desktop) throw {code: "desktopOnly"};
            const validated = validateVideoLink(input, platform);
            if (validated.error) throw {code: validated.error};
            const saved = await bridge.beforeRead(platform);
            if (current !== version || disposed) return;
            if (!saved) throw {code: "cookieSaveFailed"};
            const result = await bridge.invoke<VideoCommand>(options ? "get_video_download_command" : "get_video_parse_command",
                {platform, input: validated.url, ...(options ? {options} : {})});
            if (current === version && !disposed) command.value = result;
        } catch (failure) {
            if (current === version && !disposed) {
                error.value = typeof failure === "object" && failure !== null && "code" in failure
                    ? failure as CommandError : {code: "bridgeFailed"};
            }
        } finally {
            if (current === version) loading.value = false;
        }
    }

    async function copy() {
        if (!open.value || !command.value || copying.value || disposed) return false;
        const current = version;
        copying.value = true;
        error.value = null;
        try {
            await bridge.writeClipboard(command.value.text);
            return current === version && !disposed;
        } catch {
            if (current === version && !disposed) error.value = {code: "clipboardWriteFailed"};
            return false;
        } finally {
            if (current === version) copying.value = false;
        }
    }

    return {open, command, loading, copying, error, show, close, copy, dispose};
}
