import {getCurrentInstance, h, watch, type WatchSource} from "vue";
import {ElMessage, ElNotification, type NotificationHandle} from "element-plus";
import {useI18n} from "vue-i18n";
import {sanitizeHistoryDetail} from "./downloadHistoryDisplay.ts";
import AppNotification from "../components/AppNotification.vue";

interface FeedbackOptions {
    title?: string;
    detail?: string;
    key?: string;
    downloadOutcome?: "completed" | "failed";
}

const notifications = new Map<string, NotificationHandle>();

export function useFeedback() {
    const {t} = useI18n({useScope: "global"});
    const context = getCurrentInstance()?.appContext;

    function notify(message: string, type: "primary" | "success" | "info" | "warning" | "error", options: FeedbackOptions = {}) {
        const title = options.downloadOutcome ? t(`download.transfer.${options.downloadOutcome}`)
            : options.title ?? t(type === "error" ? "common.error" : "common.notice");
        const detail = sanitizeHistoryDetail(options.detail);
        const key = options.key ?? JSON.stringify([type, title, message, detail]);
        notifications.get(key)?.close();
        let handle: NotificationHandle;
        handle = ElNotification({
            position: "top-right", duration: 0, showClose: true, customClass: "app-notification",
            message: h(AppNotification, {
                title, type, description: detail || message,
            }),
            onClose: () => {
                if (notifications.get(key) === handle) notifications.delete(key);
            },
        }, context);
        notifications.set(key, handle);
        return handle;
    }

    function notifyError(message: string, options: FeedbackOptions = {}) {
        return notify(message, "error", options);
    }

    function notifyDownloadFailure(message: string, options: Pick<FeedbackOptions, "detail" | "key"> = {}) {
        return notifyError(message, {...options, downloadOutcome: "failed"});
    }

    function watchError<T extends { code: string; detail?: string }>(source: WatchSource<T | null | undefined>,
                                                                     message: (error: T) => string, options: (error: T) => FeedbackOptions = () => ({})) {
        return watch(source, (error, _previous, onCleanup) => {
            if (!error) return;
            const handle = notifyError(message(error), {detail: error.detail, ...options(error)});
            onCleanup(() => handle.close());
        }, {immediate: true});
    }

    function inform(message: string, type: "success" | "info" | "warning" = "success") {
        ElMessage({message, type, duration: 2000, showClose: false}, context);
    }

    return {notify, notifyError, notifyDownloadFailure, watchError, inform};
}
