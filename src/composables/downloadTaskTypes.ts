import type {DownloadRecord} from './useDownloadHistory';
import type {DownloadPageState} from './useDownloadPageState';

export type TaskPhase =
    'queued'
    | 'preparing'
    | 'downloading'
    | 'paused'
    | 'processing'
    | 'cancelling'
    | 'completed'
    | 'failed'
    | 'cancelled'
    | 'interrupted';

export interface DownloadTaskSnapshot {
    record: DownloadRecord;
    phase: TaskPhase;
    percent: number | null;
    speed: number | null;
    eta: number | null;
    submissionOrder: number;
    revision: number;
    storageError: { code: string; detail: string } | null
}

export interface SubmitDownloadRequest {
    snapshot: DownloadPageState;
    restoreTrashed: boolean;
    redownload: boolean
}

export interface SubmitDownloadResult {
    kind: 'accepted' | 'existing' | 'alreadyDownloaded' | 'confirmationRequired';
    record: DownloadRecord;
    task: DownloadTaskSnapshot | null
}

export function taskIsActive(phase: string) {
    return ['queued', 'preparing', 'downloading', 'paused', 'processing', 'cancelling'].includes(phase);
}
