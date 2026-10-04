import type {DownloadRecord} from './useDownloadHistory';
import type {createDownloadHistoryActions} from './downloadHistoryActions';

export function createHistoryFileRecovery({actions, confirmRedownload, redownload}: {
    actions: ReturnType<typeof createDownloadHistoryActions>;
    confirmRedownload: (record: DownloadRecord) => Promise<boolean>;
    redownload: (record: DownloadRecord) => Promise<unknown>
}) {
    let prompting = false;

    async function open(record: DownloadRecord, folder: boolean) {
        const ok = await (folder ? actions.openFolder(record) : actions.openFile(record));
        if (ok || actions.error.value?.code !== 'historyFileMissing' || prompting) return ok;
        prompting = true;
        try {
            if (await confirmRedownload(record)) {
                actions.clearFeedback();
                await redownload(record);
            }
        } finally {
            prompting = false;
            if (actions.error.value?.code === 'historyFileMissing') actions.clearFeedback();
        }
        return false;
    }

    return {
        openFile: (record: DownloadRecord) => open(record, false),
        openFolder: (record: DownloadRecord) => open(record, true)
    };
}
