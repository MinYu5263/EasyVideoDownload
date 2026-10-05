import {computed, ref, shallowRef} from "vue";
import {invoke, isTauri} from "@tauri-apps/api/core";
import {listen} from "@tauri-apps/api/event";
import type {LabError, LabSnapshot} from "./douyinLabTypes.ts";

interface LabBridge {
    desktop: boolean;
    invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
    listen: (receive: (snapshot: LabSnapshot) => void) => Promise<() => void>;
}

export function acceptSnapshot(current: LabSnapshot | null, incoming: LabSnapshot): LabSnapshot {
    if (current && (incoming.sessionId !== current.sessionId || incoming.revision <= current.revision)) return current;
    return incoming;
}

export function createDouyinLab(bridge: LabBridge) {
    const state = shallowRef<LabSnapshot | null>(null), ready = ref(false), connecting = ref(false),
        pending = ref(false);
    const error = ref<LabError | null>(null), connectionError = ref<LabError | null>(null);
    const selectedFormatId = ref(""), directory = ref("");
    const active = computed(() => Boolean(state.value && ["parsing", "downloading", "verifying", "cancelling"].includes(state.value.phase)));
    const busy = computed(() => active.value || pending.value);
    const selectedFormat = computed(() => state.value?.parsed?.formats.find(f => f.id === selectedFormatId.value));
    let generation = 0, unlisten: (() => void) | undefined;
    const failure = (e: unknown): LabError => typeof e === 'object' && e !== null && 'code' in e ? e as LabError : {
        code: "bridgeFailed",
        detail: String(e)
    };

    function apply(incoming: LabSnapshot, authoritative = false) {
        const old = state.value;
        if (authoritative && old && old.sessionId !== incoming.sessionId) state.value = null;
        const next = acceptSnapshot(state.value, incoming);
        state.value = next;
        if (!directory.value) directory.value = next.defaultDirectory;
        if (old?.parsed?.resultId !== next.parsed?.resultId) selectedFormatId.value = next.parsed?.formats[0]?.id ?? "";
    }

    async function refresh() {
        if (!bridge.desktop || !ready.value) return;
        const current = generation;
        try {
            const snapshot = await bridge.invoke<LabSnapshot>("douyin_lab_get_state");
            if (current === generation) {
                apply(snapshot, true);
                connectionError.value = null;
            }
        } catch (e) {
            if (current === generation) connectionError.value = failure(e);
        }
    }

    async function connect() {
        const current = ++generation;
        ready.value = false;
        connecting.value = true;
        pending.value = false;
        unlisten?.();
        unlisten = undefined;
        if (!bridge.desktop) {
            connectionError.value = {code: "desktopOnly", detail: "Use the desktop application"};
            connecting.value = false;
            return;
        }
        try {
            const stop = await bridge.listen(snapshot => {
                if (current === generation) apply(snapshot);
            });
            if (current !== generation) {
                stop();
                return;
            }
            unlisten = stop;
            const snapshot = await bridge.invoke<LabSnapshot>("douyin_lab_get_state");
            if (current !== generation) return;
            apply(snapshot, true);
            ready.value = true;
            connectionError.value = null;
        } catch (e) {
            if (current === generation) {
                connectionError.value = failure(e);
                unlisten?.();
                unlisten = undefined;
            }
        } finally {
            if (current === generation) connecting.value = false;
        }
    }

    function dispose() {
        generation++;
        unlisten?.();
        unlisten = undefined;
        ready.value = false;
        connecting.value = false;
        pending.value = false;
    }

    async function perform(command: string, args: Record<string, unknown>, allowBusy = false) {
        if (!ready.value || pending.value || (busy.value && !allowBusy)) return false;
        const current = generation;
        pending.value = true;
        error.value = null;
        try {
            const snapshot = await bridge.invoke<LabSnapshot>(command, args);
            if (current === generation) apply(snapshot);
            return true;
        } catch (e) {
            if (current === generation) error.value = failure(e);
            return false;
        } finally {
            if (current === generation) pending.value = false;
        }
    }

    const parse = (url: string) => perform("douyin_lab_parse", {url});
    const download = () => state.value?.parsed && selectedFormat.value ? perform("douyin_lab_download", {
        resultId: state.value.parsed.resultId,
        formatId: selectedFormatId.value,
        directory: directory.value
    }) : Promise.resolve(false);
    const cancel = () => state.value?.taskId ? perform("douyin_lab_cancel", {taskId: state.value.taskId}, true) : Promise.resolve(false);
    return {
        desktop: bridge.desktop,
        state,
        ready,
        connecting,
        pending,
        error,
        connectionError,
        selectedFormatId,
        selectedFormat,
        directory,
        active,
        busy,
        connect,
        refresh,
        dispose,
        parse,
        download,
        cancel
    };
}

export function useDouyinLab() {
    return createDouyinLab({
        desktop: isTauri(),
        invoke,
        listen: async receive => listen<LabSnapshot>("douyin-lab-changed", event => receive(event.payload))
    });
}
