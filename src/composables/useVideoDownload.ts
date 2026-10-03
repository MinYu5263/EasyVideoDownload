import {computed, ref} from "vue";
import type {VideoPlatform} from "./videoPlatforms.ts";
import type {DownloadCommandOptions} from "./useVideoCommand.ts";

export interface DownloadProgress {
    phase: "preparing" | "downloading" | "processing";
    percent: number | null;
    speed: number | null;
    eta: number | null;
}

interface DownloadError {
    code: string;
    detail?: string
}

interface DownloadResult {
    path: string;
    alreadyDownloaded: boolean
}

interface DownloadRequest {
    platform: VideoPlatform;
    input: string;
    options: DownloadCommandOptions
}

interface DownloadBridge {
    desktop: boolean;
    beforeDownload: (platform: VideoPlatform) => Promise<boolean>;
    invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
    createChannel: (handler: (progress: DownloadProgress) => void) => unknown;
}

let sequence = 0;

export function createVideoDownload(bridge: DownloadBridge) {
    const phase = ref<"idle" | "completed" | "failed" | "cancelled" | "cancelling" | DownloadProgress["phase"]>("idle");
    const progress = ref<DownloadProgress | null>(null);
    const percentage = ref(0);
    const result = ref<DownloadResult | null>(null);
    const error = ref<DownloadError | null>(null);
    const running = ref(false);
    const busy = computed(() => running.value);
    let requestId = "";
    let dispatched = false;
    let cancelled = false;
    let disposed = false;

    function reset() {
        if (busy.value) return;
        phase.value = "idle";
        progress.value = result.value = error.value = null;
        percentage.value = 0;
    }

    async function cancel() {
        if (!busy.value) return true;
        cancelled = true;
        phase.value = "cancelling";
        if (!dispatched) return true;
        try {
            await bridge.invoke("cancel_video_download", {requestId});
            return true;
        } catch (failure) {
            cancelled = false;
            phase.value = progress.value?.phase ?? "preparing";
            error.value = {code: "cancelFailed", detail: String(failure)};
            return false;
        }
    }

    async function start(request: DownloadRequest) {
        if (busy.value || disposed) return;
        reset();
        running.value = true;
        cancelled = false;
        dispatched = false;
        requestId = `${Date.now()}-${++sequence}`;
        const current = requestId;
        const snapshot = {...request, options: {...request.options}};
        phase.value = "preparing";
        try {
            if (!bridge.desktop) throw {code: "desktopOnly"};
            if (!snapshot.options.directory.trim()) throw {code: "invalidDownloadDirectory"};
            if (!snapshot.options.formatId) throw {code: "invalidDownloadOptions"};
            const saved = await bridge.beforeDownload(snapshot.platform);
            if (cancelled || disposed) throw {code: "downloadCancelled"};
            if (!saved) throw {code: "cookieSaveFailed"};
            const onProgress = bridge.createChannel(update => {
                if (!running.value || current !== requestId) return;
                if (cancelled) {
                    // The first native message also closes a cancel-before-registration race.
                    if (update.phase === "preparing") void cancel();
                } else if (!disposed) phase.value = update.phase;
                if (!disposed) {
                    progress.value = update;
                    if (update.percent != null && Number.isFinite(update.percent)) {
                        percentage.value = Math.max(percentage.value, Math.min(99, Math.max(0, update.percent)));
                    }
                }
            });
            dispatched = true;
            const response = await bridge.invoke<DownloadResult>("download_video", {
                ...snapshot,
                requestId: current,
                onProgress
            });
            if (disposed) return;
            result.value = response;
            error.value = null;
            phase.value = "completed";
            percentage.value = 100;
        } catch (failure) {
            if (disposed) return;
            const nativeError = typeof failure === "object" && failure !== null && "code" in failure
                ? failure as DownloadError : {code: "bridgeFailed", detail: String(failure)};
            phase.value = nativeError.code === "downloadCancelled" ? "cancelled" : "failed";
            error.value = nativeError.code === "downloadCancelled" ? null : nativeError;
        } finally {
            running.value = false;
            dispatched = false;
        }
    }

    function dispose() {
        disposed = true;
        void cancel();
    }

    return {phase, progress, percentage, result, error, busy, start, cancel, reset, dispose};
}
