import type {DownloadRecord} from './useDownloadHistory';
import type {createDownloadTasks} from './useDownloadTasks';

export function createHistoryRedownload({tasks, confirmRedownload}: {
    tasks: Pick<ReturnType<typeof createDownloadTasks>, 'redownload'>;
    confirmRedownload: (record: DownloadRecord) => Promise<boolean>
}) {
    const pending = new Map<number, Promise<boolean>>();

    function run(record: DownloadRecord): Promise<boolean> {
        if (record.deletedAt || ['queued', 'running'].includes(record.status)) return Promise.resolve(false);
        const previous = pending.get(record.id);
        if (previous) return previous;
        const request = Promise.resolve().then(async () => {
            try {
                const result = await tasks.redownload(record);
                if (result.kind !== 'confirmationRequired') return true;
                if (!await confirmRedownload(result.record)) return false;
                const restarted = await tasks.redownload(result.record, true);
                return restarted.kind === 'accepted' || restarted.kind === 'existing';
            } catch {
                return false;
            } // The shared task service retains the per-record error.
            finally {
                pending.delete(record.id);
            }
        });
        pending.set(record.id, request);
        return request;
    }

    return {run};
}
