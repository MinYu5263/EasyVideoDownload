import type {DownloadRecord} from "./useDownloadHistory.ts";
import {formatVideoSize} from "./videoFormatDisplay.ts";

export function historySize(row: Pick<DownloadRecord, "fileSizeBytes" | "selectedSizeBytes" | "sizeApproximate"> & Partial<Pick<DownloadRecord, "status">>) {
    const actual = formatVideoSize(!row.status || row.status === 'completed' ? row.fileSizeBytes : null);
    if (actual) return {kind: "actual", text: actual};
    const selected = formatVideoSize(row.selectedSizeBytes);
    return selected ? {kind: "stream", text: `${row.sizeApproximate ? "≈ " : ""}${selected}`} : {
        kind: "unknown",
        text: ""
    };
}

export function historyDuration(seconds: number | null) {
    if (seconds == null || !Number.isFinite(seconds) || seconds < 0) return null;
    const value = Math.floor(seconds), s = String(value % 60).padStart(2, "0"), m = Math.floor(value / 60);
    return m >= 60 ? `${Math.floor(m / 60)}:${String(m % 60).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

export function groupHistoryRecords(records: DownloadRecord[], now = new Date()) {
    const china = (date: Date) => new Date(date.getTime() + 8 * 3600000).toISOString().slice(0, 10);
    const today = china(now), yesterday = china(new Date(now.getTime() - 86400000));
    const groups: { date: string; label: string; records: DownloadRecord[] }[] = [];
    for (const row of records) {
        const date = row.startedAt.slice(0, 10);
        let group = groups[groups.length - 1];
        if (group?.date !== date) {
            group = {date, label: date === today ? "today" : date === yesterday ? "yesterday" : date, records: []};
            groups.push(group);
        }
        group!.records.push(row);
    }
    return groups;
}

const sensitive = /^(?:access_?token|refresh_?token|token|auth|authorization|cookie|password|passwd|secret|api_?key|signature|sig)$/i;

export function sanitizeHistoryLink(value: string) {
    try {
        const url = new URL(value);
        url.username = "";
        url.password = "";
        for (const key of [...url.searchParams.keys()]) if (sensitive.test(key)) url.searchParams.delete(key);
        return url.toString();
    } catch {
        return value;
    }
}

export function sanitizeHistoryDetail(value: string | null | undefined) {
    if (!value) return "";
    return value.slice(0, 4096)
        .replace(/\b(?:cookie|set-cookie|authorization|proxy-authorization)\s*[:=][^\r\n]*/gi, "[redacted]")
        .replace(/https?:\/\/[^\s<>"']+/gi, "[url redacted]")
        .replace(/\b(?:password|passwd|token|secret|api_key)\s*[:=]\s*[^\s,;]+/gi, "[redacted]");
}

export function historyFailureKey(row: Pick<DownloadRecord, "status" | "failureKind"> & Partial<Pick<DownloadRecord, "errorCode">>) {
    if (row.status === "interrupted") return "history.reason.interrupted";
    if (row.errorCode === "historyFileOccupied") return "history.reason.fileOccupied";
    const deletionCodes = ["historyFileDeleteFailed", "historyFilePermissionDenied", "historyFileReadOnly", "historyFileUnsafe", "historyFileChanged", "historyFileInUse", "historyFileDeletedSaveFailed"];
    if (row.errorCode && deletionCodes.includes(row.errorCode)) return `history.${row.errorCode}`;
    const kinds = ["tools", "cookie", "network", "content", "format", "filesystem", "processing", "output", "execution"];
    return `history.reason.${row.failureKind && kinds.includes(row.failureKind) ? row.failureKind : "unknown"}`;
}

export function historyStageKey(stage: string | null | undefined) {
    return `history.stage.${stage && ["preparing", "downloading", "processing", "finalizing"].includes(stage) ? stage : "unknown"}`;
}

export function historyOperationMessage(error: {
    code: string;
    detail?: string
} | null | undefined, t: (key: string, args: Record<string, string>) => string, te: (key: string) => boolean, platform: string) {
    if (!error) return null;
    const key = te(`history.${error.code}`) ? `history.${error.code}` : te(`download.errors.${error.code}`) ? `download.errors.${error.code}` :
        platform === 'douyin' && te(`download.douyinErrors.${error.code}`) ? `download.douyinErrors.${error.code}` : 'history.operationFailed';
    return t(key, {platform: t(`download.platforms.${platform}`, {})});
}

export function historySuggestionKey(row: Pick<DownloadRecord, "status" | "failureKind"> & Partial<Pick<DownloadRecord, "errorCode">>) {
    if (row.status === "interrupted") return "history.suggestion.interrupted";
    if (row.errorCode === "historyFileOccupied") return "history.suggestion.fileOccupied";
    const kinds = ["tools", "cookie", "network", "content", "format", "filesystem", "processing", "output", "execution"];
    return `history.suggestion.${row.failureKind && kinds.includes(row.failureKind) ? row.failureKind : "unknown"}`;
}
