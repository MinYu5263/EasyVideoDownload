import {createApp, nextTick, watch} from "vue";
import {getCurrentWindow} from "@tauri-apps/api/window";
import App from "./App.vue";
import {appLocale, i18n} from "./i18n";
import {useAppSettings} from "./composables/useAppSettings";
import {useUiPreferences} from "./composables/useUiPreferences";
import {useAppAppearance} from "./composables/useAppAppearance";
import "element-plus/es/components/empty/style/css";
import "element-plus/es/components/tag/style/css";
import "element-plus/es/components/alert/style/css";
import "element-plus/es/components/icon/style/css";
import "element-plus/es/components/menu/style/css";
import "element-plus/es/components/menu-item/style/css";
import "element-plus/es/components/message/style/css";
import "element-plus/es/components/notification/style/css";
import "element-plus/es/components/select/style/css";
import "element-plus/es/components/option/style/css";
import "element-plus/es/components/switch/style/css";
import "element-plus/es/components/checkbox/style/css";
import "element-plus/es/components/button/style/css";
import "element-plus/es/components/input/style/css";
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
import "element-plus/theme-chalk/dark/css-vars.css";
import "./styles/theme.css";

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
        createApp(App).use(i18n).mount("#app");
        // Flush Vue's initial DOM updates before revealing the native window.
        await nextTick();
    } catch (error) {
        console.error("Failed to initialize EasyVideoDownload", error);
        const root = document.getElementById("app");
        if (root) {
            const message = appLocale.value === "zh-CN"
                ? "应用启动失败，请重启后重试。"
                : "The application failed to start. Please restart and try again.";
            root.textContent = `${message}\n${String(error)}`;
        }
    } finally {
        // Also reveal initialization errors so the app does not stay hidden.
        if (controller.desktop) await getCurrentWindow().show();
    }
}

void bootstrap().catch(error => {
    console.error("Failed to show the EasyVideoDownload window", error);
});
