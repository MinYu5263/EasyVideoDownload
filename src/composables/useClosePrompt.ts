import {ref} from "vue";
import type {createAppSettings} from "./useAppSettings.ts";

type CloseResponse = "exit" | "tray" | "cancel";

interface ClosePromptBridge {
    desktop: boolean;
    listen: (handler: () => void) => Promise<() => void>;
    invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
}

type Settings = Pick<ReturnType<typeof createAppSettings>, "update" | "saveError">;

export function createClosePrompt(bridge: ClosePromptBridge, settings: Settings) {
    const open = ref(false), remember = ref(false);
    const busy = ref<CloseResponse | null>(null);
    const error = ref<{ code: "listenFailed" | "saveFailed" | "actionFailed"; detail: string } | null>(null);
    let unlisten: (() => void) | undefined;
    let generation = 0, responseRevision = 0, requestRevision = 0, disposed = false;

    function show(newRequest: boolean) {
        if (disposed) return;
        if (newRequest) ++requestRevision;
        if (open.value) return;
        remember.value = false;
        error.value = null;
        open.value = true;
    }

    async function start() {
        if (!bridge.desktop) return;
        disposed = false;
        const current = ++generation;
        const observedResponse = responseRevision;
        try {
            if (!unlisten) {
                const stop = await bridge.listen(() => show(true));
                if (disposed || current !== generation) {
                    stop();
                    return;
                }
                unlisten = stop;
            }
            // Recover a close request that arrived before mounting or during HMR.
            const pending = await bridge.invoke<boolean>("get_close_prompt_state");
            if (disposed || current !== generation || observedResponse !== responseRevision) return;
            error.value = null;
            if (pending) show(false);
        } catch (failure) {
            if (!disposed && current === generation) error.value = {code: "listenFailed", detail: String(failure)};
        }
    }

    async function choose(action: CloseResponse): Promise<boolean> {
        if (disposed || !bridge.desktop || !open.value || busy.value) return false;
        ++responseRevision;
        const handledRequest = requestRevision;
        busy.value = action;
        error.value = null;
        try {
            if (action !== "cancel" && remember.value) {
                if (!await settings.update({closeAction: action})) {
                    error.value = {code: "saveFailed", detail: settings.saveError.value?.detail ?? ""};
                    return false;
                }
            }
            if (disposed) return false;
            await bridge.invoke("respond_to_close_request", {action});
            // A newer native request can arrive before this IPC reply resolves.
            if (handledRequest === requestRevision) open.value = false;
            remember.value = false;
            return true;
        } catch (failure) {
            error.value = {code: "actionFailed", detail: String(failure)};
            return false;
        } finally {
            busy.value = null;
        }
    }

    function dispose() {
        disposed = true;
        ++generation;
        unlisten?.();
        unlisten = undefined;
    }

    return {open, remember, busy, error, start, choose, dispose};
}
