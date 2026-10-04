import {Clock, Download, Setting} from "@element-plus/icons-vue";

export const appPages = [
    {
        id: "download",
        labelKey: "navigation.download",
        icon: Download,
        emptyTitleKey: "pages.download.emptyTitle",
        emptyDescriptionKey: "pages.download.emptyDescription",
    },
    {
        id: "history",
        labelKey: "navigation.history",
        icon: Clock,
        emptyTitleKey: "pages.history.emptyTitle",
        emptyDescriptionKey: "pages.history.emptyDescription",
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
