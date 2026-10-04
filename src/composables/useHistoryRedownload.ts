import type {DownloadRecord} from './useDownloadHistory';
import type {createDownloadTasks} from './useDownloadTasks';

export function createHistoryRedownload({tasks}: {
    tasks: Pick<ReturnType<typeof createDownloadTasks>, 'redownload'>
}) {
    const pending = new Map<number, Promise<boolean>>();

    function run(record: DownloadRecord): Promise<boolean> {
        if (record.deletedAt || ['queued', 'running'].includes(record.status)) return Promise.resolve(false);
        const previous = pending.get(record.id);
        if (previous) return previous;
        const request = Promise.resolve().then(async () => {
            try {
                await tasks.redownload(record);
                return true;
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
