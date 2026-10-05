import {onMounted, reactive, ref} from "vue";
import {Channel, invoke, isTauri} from "@tauri-apps/api/core";

export const toolIds = ["ytdlp", "ffmpeg", "deno"] as const;
export type RequiredToolId = (typeof toolIds)[number];
export type RequiredToolSource = "path" | "manual" | "automatic";
// Notify parsed-video caches only when an applied yt-dlp/Deno configuration changes.
export const videoToolRevision = ref(0);

export interface RequiredToolError {
    code: string;
    program: string;
    detail: string
}

export interface RequiredToolConfig {
    source: RequiredToolSource;
    manualPath: string;
    programs: { name: string; path: string; version: string }[];
    checkedAt: string;
}

export interface RequiredToolState {
    source: RequiredToolSource;
    manualPath: string;
    active: RequiredToolConfig | null;
    operation: "checking" | "choosing" | "configuring" | null;
    error: RequiredToolError | null;
    progress: ConfigureProgress | null;
    cancelling: boolean;
    cancellationUnavailable: boolean;
}

type RequiredToolCheckState = Pick<RequiredToolState, "source" | "manualPath" | "error">;

export interface ConfigureProgress {
    phase: "preparing" | "downloading" | "verifying" | "extracting" | "checking" | "saving";
    downloaded: number;
    total: number | null;
}

interface RequiredToolBridge {
    desktop: boolean;
    invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
}

function configurationKey(config: RequiredToolConfig | null) {
    return config && JSON.stringify({
        source: config.source, manualPath: config.manualPath,
        programs: config.programs.map(program => ({name: program.name, path: program.path, version: program.version}))
    });
}

function bridgeError(value: unknown): RequiredToolError {
    if (typeof value === "object" && value !== null && "code" in value) return value as RequiredToolError;
    return {code: "bridgeFailed", program: "", detail: String(value)};
}

export function createRequiredTools(bridge: RequiredToolBridge) {
    const ready = ref(false);
    const automaticSupported = reactive<Record<RequiredToolId, boolean>>({ytdlp: false, ffmpeg: false, deno: false});
    const automaticFfmpegRequiresRosetta = ref(false);
    const loadError = ref<RequiredToolError | null>(null);
    const tools = reactive(Object.fromEntries(toolIds.map(id => [id, {
        source: "path",
        manualPath: "",
        active: null,
        operation: null,
        error: null,
        progress: null,
        cancelling: false,
        cancellationUnavailable: false,
    }])) as Record<RequiredToolId, RequiredToolState>);
    let pendingLoad = false;
    let loadTask: Promise<void> | undefined;

    async function load() {
        if (!bridge.desktop) return;
        if (loadTask) return loadTask;
        if (toolIds.some(id => tools[id].operation !== null)) {
            pendingLoad = true;
            return;
        }
        pendingLoad = false;
        ready.value = false;
        // Keep cards locked until the shared read completes, so an older refresh
        // cannot publish its snapshot after a successful check or configuration.
        loadTask = (async () => {
            try {
                const snapshot = await bridge.invoke<{
                    settings: { tools: Partial<Record<RequiredToolId, RequiredToolConfig>> };
                    lastChecks?: Partial<Record<RequiredToolId, RequiredToolCheckState>>;
                    error: RequiredToolError | null
                    automaticSupported: Partial<Record<RequiredToolId, boolean>>;
                    automaticFfmpegRequiresRosetta: boolean;
                }>("get_required_tools");
                loadError.value = snapshot.error;
                for (const id of toolIds) automaticSupported[id] = snapshot.automaticSupported?.[id] === true;
                automaticFfmpegRequiresRosetta.value = snapshot.automaticFfmpegRequiresRosetta === true;
                for (const id of toolIds) {
                    const active = snapshot.settings.tools[id] ?? null;
                    const lastCheck = snapshot.lastChecks?.[id];
                    Object.assign(tools[id], {
                        source: lastCheck?.source ?? active?.source ?? "path",
                        manualPath: lastCheck?.manualPath ?? active?.manualPath ?? "",
                        active, error: lastCheck?.error ?? null,
                    });
                }
                ready.value = snapshot.error === null;
            } catch (error) {
                loadError.value = bridgeError(error);
            } finally {
                loadTask = undefined;
            }
        })();
        return loadTask;
    }

    async function loadWhenIdle() {
        if (pendingLoad && toolIds.every(id => tools[id].operation === null)) await load();
    }

    async function check(id: RequiredToolId) {
        const tool = tools[id];
        if (!ready.value || tool.operation) return;
        tool.operation = "checking";
        tool.error = null;
        const previous = configurationKey(tool.active);
        try {
            const result = await bridge.invoke<{
                active: RequiredToolConfig | null;
                error: RequiredToolError | null
            }>("check_required_tool", {
                request: {toolId: id, source: tool.source, manualPath: tool.manualPath},
            });
            tool.active = result.active;
            tool.error = result.error;
            if (!result.error && result.active) tool.manualPath = result.active.manualPath;
            if (id !== "ffmpeg" && !result.error && configurationKey(result.active) !== previous) videoToolRevision.value += 1;
        } catch (error) {
            tool.error = bridgeError(error);
        } finally {
            tool.operation = null;
            await loadWhenIdle();
        }
    }

    async function choose(id: RequiredToolId) {
        const tool = tools[id];
        if (!ready.value || tool.operation) return;
        tool.operation = "choosing";
        let selected = false;
        try {
            const path = await bridge.invoke<string | null>("select_required_tool_path", {toolId: id});
            if (path !== null) {
                tool.source = "manual";
                tool.manualPath = path;
                selected = true;
            }
        } catch (error) {
            tool.error = bridgeError(error);
        } finally {
            tool.operation = null;
        }
        if (selected) await check(id);
        await loadWhenIdle();
    }

    async function changeSource(id: RequiredToolId, source: RequiredToolSource) {
        if (!ready.value || tools[id].operation) return;
        if (source === "automatic" && !automaticSupported[id]) return;
        tools[id].source = source;
        tools[id].error = null;
        if (source === "manual" && !tools[id].manualPath.trim()) await choose(id);
        else await check(id);
    }

    async function configure(id: RequiredToolId) {
        const tool = tools[id];
        if (!ready.value || tool.operation || !automaticSupported[id] || tool.source !== "automatic") return;
        tool.operation = "configuring";
        tool.error = null;
        tool.progress = null;
        tool.cancellationUnavailable = false;
        const previous = configurationKey(tool.active);
        const onProgress = new Channel<ConfigureProgress>();
        onProgress.onmessage = progress => {
            if (tool.operation === "configuring") tool.progress = progress;
        };
        try {
            const result = await bridge.invoke<{
                active: RequiredToolConfig | null;
                error: RequiredToolError | null;
            }>("configure_required_tool", {toolId: id, onProgress});
            tool.active = result.active;
            tool.error = result.error;
            if (!result.error && result.active) {
                tool.manualPath = result.active.manualPath;
            }
            // A saved configuration can also carry a staging-cleanup error.
            if (id !== "ffmpeg" && configurationKey(result.active) !== previous) videoToolRevision.value += 1;
        } catch (error) {
            tool.error = bridgeError(error);
        } finally {
            tool.operation = null;
            tool.progress = null;
            tool.cancelling = false;
            await loadWhenIdle();
        }
    }

    async function cancelConfiguration(id: RequiredToolId) {
        const tool = tools[id];
        if (tool.operation !== "configuring" || !tool.progress || tool.progress.phase === "saving" || tool.cancelling || tool.cancellationUnavailable) return;
        tool.cancelling = true;
        try {
            const accepted = await bridge.invoke<boolean>("cancel_tool_configuration", {toolId: id});
            if (!accepted && tool.operation === "configuring") {
                tool.cancelling = false;
                tool.cancellationUnavailable = true;
            }
        } catch (error) {
            tool.error = bridgeError(error);
            tool.cancelling = false;
        }
    }

    return {
        desktop: bridge.desktop,
        ready,
        loadError,
        automaticSupported,
        automaticFfmpegRequiresRosetta,
        tools,
        load,
        check,
        choose,
        changeSource,
        configure,
        cancelConfiguration
    };
}

export function useRequiredTools() {
    const controller = createRequiredTools({desktop: isTauri(), invoke});
    onMounted(controller.load);
    return controller;
}
