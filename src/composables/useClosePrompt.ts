import {ref} from "vue";
import type {createAppSettings} from "./useAppSettings.ts";

type CloseResponse = "exit" | "tray" | "cancel";

export interface ClosePromptState {
    revision: number;
    open: boolean;
    allowBackground: boolean;
    hasActiveTasks: boolean;
}

interface ClosePromptBridge {
    desktop: boolean;
    listen: (handler: (state: ClosePromptState) => void) => Promise<() => void>;
    invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
}

type Settings = Pick<ReturnType<typeof createAppSettings>, "update" | "saveError">;

export function createClosePrompt(bridge: ClosePromptBridge, settings: Settings) {
    const open = ref(false), remember = ref(false);
    const allowBackground = ref(true), hasActiveTasks = ref(false);
    const busy = ref<CloseResponse | null>(null);
    const error = ref<{ code: "listenFailed" | "saveFailed" | "actionFailed"; detail: string } | null>(null);
    let unlisten: (() => void) | undefined;
    let generation = 0, responseRevision = 0, requestRevision = 0, disposed = false;
    let closedRevision = 0;

    function show(state: ClosePromptState) {
        if (disposed || !state.open || state.revision <= closedRevision || state.revision < requestRevision) return;
        requestRevision = state.revision;
        allowBackground.value = state.allowBackground;
        hasActiveTasks.value = state.hasActiveTasks;
        if (!state.allowBackground) remember.value = false;
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
        const observedRequest = requestRevision;
        try {
            if (!unlisten) {
                const stop = await bridge.listen(show);
                if (disposed || current !== generation) {
                    stop();
                    return;
                }
                unlisten = stop;
            }
            // Recover a close request that arrived before mounting or during HMR.
            const pending = await bridge.invoke<ClosePromptState>("get_close_prompt_state");
            if (disposed || current !== generation || observedResponse !== responseRevision || observedRequest !== requestRevision) return;
            error.value = null;
            if (pending.open) show(pending);
            else closedRevision = Math.max(closedRevision, pending.revision);
        } catch (failure) {
            if (!disposed && current === generation) error.value = {code: "listenFailed", detail: String(failure)};
        }
    }

    async function choose(action: CloseResponse): Promise<boolean> {
        if (disposed || !bridge.desktop || !open.value || busy.value) return false;
        if (action === "tray" && !allowBackground.value) return false;
        ++responseRevision;
        const handledRequest = requestRevision;
        busy.value = action;
        error.value = null;
        try {
            if (action !== "cancel" && allowBackground.value && remember.value) {
                if (!await settings.update({closeAction: action})) {
                    error.value = {code: "saveFailed", detail: settings.saveError.value?.detail ?? ""};
                    return false;
                }
            }
            if (disposed || handledRequest !== requestRevision) return false;
            const pending = await bridge.invoke<ClosePromptState>("respond_to_close_request", {
                action,
                revision: handledRequest
            });
            if (pending.open) {
                show(pending);
                return false;
            }
            closedRevision = Math.max(closedRevision, pending.revision);
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

    return {open, remember, allowBackground, hasActiveTasks, busy, error, start, choose, dispose};
}
