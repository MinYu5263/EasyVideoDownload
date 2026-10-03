import {createApp, watch} from "vue";
import App from "./App.vue";
import {appLocale, i18n} from "./i18n";
import {useAppSettings} from "./composables/useAppSettings";
import "element-plus/es/components/empty/style/css";
import "element-plus/es/components/icon/style/css";
import "element-plus/es/components/menu/style/css";
import "element-plus/es/components/menu-item/style/css";
import "element-plus/es/components/message/style/css";
import "element-plus/es/components/select/style/css";
import "element-plus/es/components/option/style/css";
import "element-plus/es/components/switch/style/css";
import "element-plus/es/components/button/style/css";
import "element-plus/es/components/input/style/css";
import "./styles/theme.css";

async function bootstrap() {
    const controller = useAppSettings();
    await controller.load(appLocale.value);
    watch(() => controller.settings.locale, locale => {
        appLocale.value = locale;
    }, {immediate: true});
    if (controller.desktop && controller.ready.value) {
        try {
            // SQLite is authoritative after successful initialization/migration.
            localStorage.removeItem("easyvideodownload.locale");
        } catch {
            // An inaccessible legacy store must not prevent the app from starting.
        }
    }
    createApp(App).use(i18n).mount("#app");
}

void bootstrap();
