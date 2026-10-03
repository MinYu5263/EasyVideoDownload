import {computed, watch} from "vue";
import {createI18n} from "vue-i18n";
import elementEn from "element-plus/es/locale/lang/en";
import elementZhCN from "element-plus/es/locale/lang/zh-cn";
import en from "./locales/en";
import zhCN, {type MessageSchema} from "./locales/zh-CN";

export type AppLocale = "zh-CN" | "en";

const localeStorageKey = "easyvideodownload.locale";

export const languageOptions = [
    {value: "zh-CN", labelKey: "languages.simplifiedChinese"},
    {value: "en", labelKey: "languages.english"},
] as const;

function getInitialLocale(): AppLocale {
    try {
        const savedLocale = localStorage.getItem(localeStorageKey);
        if (savedLocale === "zh-CN" || savedLocale === "en") return savedLocale;
    } catch {
        // If storage is unavailable, use the system language for this session.
    }

    return navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
}

export const i18n = createI18n<[MessageSchema], AppLocale, false>({
    legacy: false,
    globalInjection: false,
    locale: getInitialLocale(),
    fallbackLocale: "zh-CN",
    messages: {
        "zh-CN": zhCN,
        en,
    },
});

export const appLocale = computed<AppLocale>({
    get: () => i18n.global.locale.value,
    set: (value) => {
        i18n.global.locale.value = value;
    },
});

export const elementPlusLocale = computed(() =>
    appLocale.value === "zh-CN" ? elementZhCN : elementEn,
);

watch(
    i18n.global.locale,
    (locale, previousLocale) => {
        document.documentElement.lang = locale;
        if (previousLocale === undefined) return;

        try {
            localStorage.setItem(localeStorageKey, locale);
        } catch {
            // Language switching still works when storage is unavailable.
        }
    },
    {immediate: true},
);
