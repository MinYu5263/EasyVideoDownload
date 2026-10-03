import {computed, onMounted, reactive, ref} from "vue";
import {invoke, isTauri} from "@tauri-apps/api/core";

export const toolIds = ["ytdlp", "ffmpeg", "deno"] as const;
export type RequiredToolId = (typeof toolIds)[number];
export type RequiredToolSource = "path" | "manual";
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
    operation: "checking" | "choosing" | null;
    error: RequiredToolError | null;
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
    const loadError = ref<RequiredToolError | null>(null);
    const tools = reactive(Object.fromEntries(toolIds.map(id => [id, {
        source: "path", manualPath: "", active: null, operation: null, error: null,
    }])) as Record<RequiredToolId, RequiredToolState>);
    const busy = computed(() => toolIds.some(id => tools[id].operation !== null));

    async function load() {
        if (!bridge.desktop) return;
        try {
            const snapshot = await bridge.invoke<{
                settings: { tools: Partial<Record<RequiredToolId, RequiredToolConfig>> };
                error: RequiredToolError | null
            }>("get_required_tools");
            loadError.value = snapshot.error;
            for (const id of toolIds) {
                const active = snapshot.settings.tools[id];
                if (active) Object.assign(tools[id], {source: active.source, manualPath: active.manualPath, active});
            }
            ready.value = snapshot.error === null;
        } catch (error) {
            loadError.value = bridgeError(error);
        }
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
    }

    async function changeSource(id: RequiredToolId, source: RequiredToolSource) {
        if (!ready.value || tools[id].operation) return;
        tools[id].source = source;
        if (source === "manual" && !tools[id].manualPath.trim()) await choose(id);
        else await check(id);
    }

    async function checkAll() {
        if (busy.value || !ready.value) return;
        await Promise.allSettled(toolIds.map(check));
    }

    return {desktop: bridge.desktop, ready, loadError, tools, busy, load, check, choose, changeSource, checkAll};
}

export function useRequiredTools() {
    const controller = createRequiredTools({desktop: isTauri(), invoke});
    onMounted(controller.load);
    return controller;
}
