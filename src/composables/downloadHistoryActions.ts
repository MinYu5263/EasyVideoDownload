import {ref} from "vue";
import {invoke} from "@tauri-apps/api/core";
import {useDesktopActions} from "./useDesktopActions.ts";
import {persistenceError, type PersistenceError} from "./useUiPreferences.ts";
import type {DownloadRecord} from "./useDownloadHistory.ts";
import {sanitizeHistoryLink} from "./downloadHistoryDisplay.ts";

interface HistoryActionBridge {
    desktop: boolean;
    invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
    writeClipboard: (text: string) => Promise<void>;
}

export interface DeleteFileResult {
    fileDeleted: boolean
}

export function createDownloadHistoryActions(bridge: HistoryActionBridge) {
    const error = ref<PersistenceError | null>(null), busy = ref(false), success = ref<string | null>(null);

    async function perform(action: () => Promise<unknown>, message: string | null = null) {
        if (busy.value) return false;
        if (!bridge.desktop) {
            error.value = {code: "desktopOnly"};
            return false;
        }
        busy.value = true;
        success.value = null;
        try {
            await action();
            error.value = null;
            success.value = message;
            return true;
        } catch (e) {
            error.value = persistenceError(e);
            return false;
        } finally {
            busy.value = false;
        }
    }

    const native = (command: string, row: Pick<DownloadRecord, "id">) => perform(() => bridge.invoke(command, {id: row.id}));

    function remove(row: Pick<DownloadRecord, "id" | "status">) {
        if (["queued", "running"].includes(row.status)) {
            error.value = {code: "recordRunning"};
            return Promise.resolve(false);
        }
        return native("delete_download_record", row);
    }

    function purge(row: Pick<DownloadRecord, "id" | "status" | "deletedAt">, downloading = false) {
        if (downloading) {
            error.value = {code: "historyBusy"};
            return Promise.resolve(false);
        }
        if (["queued", "running"].includes(row.status)) {
            error.value = {code: "recordRunning"};
            return Promise.resolve(false);
        }
        if (!row.deletedAt) {
            error.value = {code: "recordNotTrashed"};
            return Promise.resolve(false);
        }
        return native("purge_download_record", row);
    }

    function emptyTrash(downloading = false) {
        if (downloading) {
            error.value = {code: "historyBusy"};
            return Promise.resolve(false);
        }
        return perform(() => bridge.invoke("empty_download_record_trash"));
    }

    async function removeAndFile(row: Pick<DownloadRecord, "id" | "status" | "deletedAt" | "outputPath">, downloading = false): Promise<DeleteFileResult | null> {
        if (downloading) {
            error.value = {code: "historyBusy"};
            return null;
        }
        if (["queued", "running"].includes(row.status)) {
            error.value = {code: "recordRunning"};
            return null;
        }
        if (row.deletedAt) {
            error.value = {code: "recordTrashed"};
            return null;
        }
        if (row.status !== "completed" || !row.outputPath) {
            error.value = {code: "historyFileUnavailable"};
            return null;
        }
        let result: DeleteFileResult | null = null;
        const ok = await perform(async () => {
            result = await bridge.invoke<DeleteFileResult>("delete_download_record_and_file", {id: row.id});
        });
        return ok ? result : null;
    }

    return {
        desktop: bridge.desktop, error, busy, success, remove, purge, removeAndFile, emptyTrash,
        restore: (row: Pick<DownloadRecord, "id">) => native("restore_download_record", row),
        openFile: (row: Pick<DownloadRecord, "id">) => native("open_download_record_file", row),
        openFolder: (row: Pick<DownloadRecord, "id">) => native("open_download_record_folder", row),
        openSource: (row: Pick<DownloadRecord, "id">) => native("open_download_record_source", row),
        copyLink: (row: Pick<DownloadRecord, "sourceLink">) => perform(() => bridge.writeClipboard(sanitizeHistoryLink(row.sourceLink)), "linkCopied"),
        clearFeedback: () => {
            error.value = null;
            success.value = null;
        }
    };
}

export function useDownloadHistoryActions() {
    const desktop = useDesktopActions();
    return createDownloadHistoryActions({desktop: desktop.desktop, invoke, writeClipboard: desktop.writeClipboard});
}
