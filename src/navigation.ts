import {Clock, Download, Headset, Setting} from "@element-plus/icons-vue";

export const appPages = [
    {
        id: "download",
        labelKey: "navigation.download",
        icon: Download,
        emptyTitleKey: "pages.download.emptyTitle",
        emptyDescriptionKey: "pages.download.emptyDescription",
    },
    {
        id: "audio",
        labelKey: "navigation.audio",
        icon: Headset,
        emptyTitleKey: "navigation.audio",
    },
    {
        id: "history",
        labelKey: "navigation.history",
        icon: Clock,
        emptyTitleKey: "pages.history.emptyTitle",
    },
    {
        id: "settings",
        labelKey: "navigation.settings",
        icon: Setting,
        emptyTitleKey: "pages.settings.emptyTitle",
        emptyDescriptionKey: "pages.settings.emptyDescription",
    },
] as const;

export type AppPageId = (typeof appPages)[number]["id"];
export type PersistedAppPageId = AppPageId;
