import { createApp } from "vue";
import App from "./App.vue";
import { i18n } from "./i18n";
import "element-plus/es/components/empty/style/css";
import "element-plus/es/components/icon/style/css";
import "element-plus/es/components/menu/style/css";
import "element-plus/es/components/menu-item/style/css";
import "element-plus/es/components/select/style/css";
import "element-plus/es/components/option/style/css";
import "element-plus/es/components/button/style/css";
import "element-plus/es/components/input/style/css";
import "./styles/theme.css";

createApp(App).use(i18n).mount("#app");
