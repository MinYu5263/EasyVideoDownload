import {createApp, h, nextTick, watch} from "vue";
import {ElNotification} from "element-plus";
import {getCurrentWindow} from "@tauri-apps/api/window";
import {error as writeLogError} from "@tauri-apps/plugin-log";
import {ElLoading} from "element-plus";
import App from "./App.vue";
import AppNotification from "./components/AppNotification.vue";
import {appLocale, i18n} from "./i18n";
import {useAppSettings} from "./composables/useAppSettings";
import {useUiPreferences} from "./composables/useUiPreferences";
import {useAppAppearance} from "./composables/useAppAppearance";
import {sanitizeHistoryDetail} from "./composables/downloadHistoryDisplay";
import "element-plus/es/components/empty/style/css";
import "element-plus/es/components/tag/style/css";
import "element-plus/es/components/alert/style/css";
import "element-plus/es/components/icon/style/css";
import "element-plus/es/components/menu/style/css";
import "element-plus/es/components/menu-item/style/css";
import "element-plus/es/components/message/style/css";
import "element-plus/es/components/notification/style/css";
import "element-plus/es/components/text/style/css";
import "element-plus/es/components/select/style/css";
import "element-plus/es/components/option/style/css";
import "element-plus/es/components/switch/style/css";
import "element-plus/es/components/checkbox/style/css";
import "element-plus/es/components/table/style/css";
import "element-plus/es/components/radio-group/style/css";
import "element-plus/es/components/button/style/css";
import "element-plus/es/components/input/style/css";
import "element-plus/es/components/input-number/style/css";
import "element-plus/es/components/dialog/style/css";
import "element-plus/es/components/drawer/style/css";
import "element-plus/es/components/dropdown/style/css";
import "element-plus/es/components/dropdown-menu/style/css";
import "element-plus/es/components/dropdown-item/style/css";
import "element-plus/es/components/message-box/style/css";
import "element-plus/es/components/tooltip/style/css";
import "element-plus/es/components/skeleton/style/css";
import "element-plus/es/components/skeleton-item/style/css";
import "element-plus/es/components/progress/style/css";
import "element-plus/es/components/scrollbar/style/css";
import "element-plus/es/components/loading/style/css";
import "element-plus/theme-chalk/dark/css-vars.css";
import "./styles/theme.css";

document.addEventListener("contextmenu", event => event.preventDefault());

function reportStartupError(context: string, cause: unknown, desktop: boolean) {
    if (!desktop) {
        console.error(context, cause);
        return;
    }
    let detail: string;
    try {
        detail = typeof cause === "object" && cause !== null && "stack" in cause
            ? String(cause.stack)
            : typeof cause === "string" ? cause : JSON.stringify(cause) ?? String(cause);
    } catch {
        detail = String(cause);
    }
    void writeLogError(JSON.stringify({event: "frontendError", context, detail}), {file: "src/main.ts"})
        .catch(() => console.error("Failed to write an application diagnostic"));
}

async function bootstrap() {
    const controller = useAppSettings();
    try {
        await Promise.all([controller.load(appLocale.value), useUiPreferences().load()]);
        watch(() => controller.settings.locale, locale => {
            appLocale.value = locale;
        }, {immediate: true});
        const appearance = useAppAppearance();
        await appearance.start(controller.settings.theme);
        watch(() => controller.settings.theme, theme => {
            void appearance.setTheme(theme);
        });
        if (controller.desktop && controller.ready.value) {
            try {
                // SQLite is authoritative after successful initialization/migration.
                localStorage.removeItem("easyvideodownload.locale");
            } catch {
                // An inaccessible legacy store must not prevent the app from starting.
            }
        }
        createApp(App).use(ElLoading).use(i18n).mount("#app");
        // Flush Vue's initial DOM updates before revealing the native window.
        await nextTick();
    } catch (error) {
        reportStartupError("Failed to initialize EasyVideoDownload", error, controller.desktop);
        ElNotification({
            duration: 0, showClose: true, customClass: 'app-notification',
            message: h(AppNotification, {
                type: 'error', title: i18n.global.t('common.error'),
                description: sanitizeHistoryDetail(String(error)) || i18n.global.t('common.startupFailed'),
            }),
        });
    } finally {
        // Also reveal initialization errors so the app does not stay hidden.
        if (controller.desktop) await getCurrentWindow().show();
    }
}

void bootstrap().catch(error => {
    reportStartupError("Failed to show the EasyVideoDownload window", error, useAppSettings().desktop);
    ElNotification({
        duration: 0, showClose: true, customClass: 'app-notification',
        message: h(AppNotification, {
            type: 'error', title: i18n.global.t('common.error'),
            description: sanitizeHistoryDetail(String(error)) || i18n.global.t('common.windowShowFailed'),
        }),
    });
});
