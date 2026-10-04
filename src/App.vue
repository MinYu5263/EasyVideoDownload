<script setup lang="ts">
import {computed, onMounted, onUnmounted, ref} from "vue";
import {ElConfigProvider} from "element-plus";
import {useI18n} from "vue-i18n";
import AppSidebar from "./components/AppSidebar.vue";
import DownloadPage from "./components/DownloadPage.vue";
import SettingsPage from "./components/SettingsPage.vue";
import HistoryPage from "./components/HistoryPage.vue";
import CloseWindowDialog from "./components/CloseWindowDialog.vue";
import {elementPlusLocale} from "./i18n";
import {type AppPageId, appPages} from "./navigation";
import {useUiPreferences} from "./composables/useUiPreferences";
import {useAppSettings} from "./composables/useAppSettings";
import {useAppAppearance} from "./composables/useAppAppearance";
import {listen, type UnlistenFn} from "@tauri-apps/api/event";
import {ElButton} from "element-plus";
import {useDownloadHistory} from "./composables/useDownloadHistory";

const uiPreferences = useUiPreferences();
const appearance = useAppAppearance();
const appSettings = useAppSettings();
const nativeError = ref<{ code: string; detail: string } | null>(null);
let unlistenNativeError: UnlistenFn | undefined;
let disposed = false;
onMounted(async () => {
  if (!appSettings.desktop) return;
  try {
    const unlisten = await listen<{ code: string; detail: string }>("app-native-error", event => {
      nativeError.value = event.payload;
    });
    if (disposed) unlisten();
    else unlistenNativeError = unlisten;
  } catch (error) {
    nativeError.value = {code: "listenFailed", detail: String(error)};
  }
});
onUnmounted(() => {
  disposed = true;
  unlistenNativeError?.();
});
import {useDownloadTasks} from "./composables/useDownloadTasks";

const tasks = useDownloadTasks();
onMounted(tasks.connect);
onUnmounted(tasks.dispose);
const historyPage = ref<InstanceType<typeof HistoryPage>>();

async function viewHistory(id: number) {
  activePageId.value = "history";
  await historyPage.value?.revealRecord(id);
}

const history = useDownloadHistory();
const downloadBusy = ref(true);
onMounted(history.refresh);
onUnmounted(history.dispose);
const activePageId = computed({
  get: () => uiPreferences.draft.activePage, set: (value: AppPageId) => {
    void uiPreferences.update({activePage: value});
  }
});

async function retryPreferences() {
  if (!uiPreferences.ready.value) await uiPreferences.load();
  else await uiPreferences.update({...uiPreferences.draft});
}
const {t} = useI18n({useScope: "global"});
const activePage = computed(
    () => appPages.find((page) => page.id === activePageId.value) ?? appPages[0],
);
</script>

<template>
  <ElConfigProvider :locale="elementPlusLocale">
    <a class="skip-link" href="#main-content">
      {{ t("accessibility.skipToContent") }}
    </a>

    <div class="app-shell">
      <AppSidebar v-model="activePageId"/>

      <main id="main-content" class="app-main" tabindex="-1">
        <header class="page-heading">
          <h1 id="page-title">{{ t(activePage.labelKey) }}</h1>
        </header>
        <div v-if="uiPreferences.loadError.value || uiPreferences.saveError.value" class="persistence-error"
             role="alert">
          {{ t(uiPreferences.loadError.value ? 'persistence.loadFailed' : 'persistence.saveFailed') }}
          <ElButton size="small" @click="retryPreferences">{{ t('persistence.retry') }}</ElButton>
        </div>
        <div v-if="tasks.error.value" class="persistence-error" role="alert">
          {{ t('tasks.connectionFailed') }}
          <ElButton size="small" @click="tasks.connect">{{ t('persistence.retry') }}</ElButton>
        </div>
        <div v-if="history.connectionError.value" class="persistence-error" role="alert">
          {{ t('history.listenFailed') }}
          <ElButton size="small" @click="history.refresh">{{ t('persistence.retry') }}</ElButton>
        </div>
        <div v-if="appearance.error.value" class="persistence-error" role="alert">
          {{ t('settings.theme.applyFailed') }}
          <ElButton size="small" @click="appearance.start(appSettings.settings.theme)">{{
              t('persistence.retry')
            }}
          </ElButton>
        </div>
        <div v-if="nativeError" class="persistence-error" role="alert">
          {{ t(`settings.nativeErrors.${nativeError.code}`) }}
          <ElButton size="small" @click="nativeError = null">{{ t('download.command.close') }}</ElButton>
        </div>

        <DownloadPage v-show="activePage.id === 'download'" :active="activePage.id === 'download'"
                      @busy-change="downloadBusy = $event" @view-history="viewHistory"/>
        <SettingsPage v-show="activePage.id === 'settings'"/>
        <HistoryPage v-show="activePage.id === 'history'" ref="historyPage" :active="activePage.id === 'history'"
                     :downloading="downloadBusy"/>
        <CloseWindowDialog/>
      </main>
    </div>
  </ElConfigProvider>
</template>

<style scoped>
.skip-link {
  position: fixed;
  top: 12px;
  left: 12px;
  z-index: 10;
  padding: 10px 16px;
  border-radius: 8px;
  background: var(--app-accent);
  color: var(--app-accent-contrast);
  transform: translateY(-160%);
}

.skip-link:focus {
  transform: translateY(0);
}

.app-shell {
  display: grid;
  grid-template-columns: var(--app-sidebar-width) minmax(0, 1fr);
  height: 100%;
  overflow: hidden;
}

.app-main {
  --app-page-padding-x: 32px;
  --app-page-padding-bottom: 26px;

  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
  padding-top: 30px;
}

.app-main:focus {
  outline: none;
}

.page-heading {
  flex-shrink: 0;
  margin: 0 var(--app-page-padding-x) 24px;
}

.page-scrollbar {
  flex: 1;
  min-height: 0;
}

.page-scrollbar :deep(.page-content) {
  padding: 0 var(--app-page-padding-x) var(--app-page-padding-bottom);
}

.page-heading h1 {
  margin: 0;
  color: var(--app-text);
  font-size: 24px;
  font-weight: 650;
  line-height: 1.4;
  letter-spacing: -0.6px;
}

.page-placeholder {
  display: grid;
  min-height: 280px;
  place-items: center;
  padding: 24px;
  border: 1px dashed var(--app-border);
  border-radius: var(--app-radius);
}

.placeholder-icon {
  display: grid;
  width: 56px;
  height: 56px;
  place-items: center;
  border-radius: 50%;
  background: var(--app-accent-soft);
  color: var(--app-accent);
}

.placeholder-icon :deep(svg) {
  color: var(--app-accent);
}

.page-placeholder h2 {
  margin: 0 0 8px;
  color: var(--app-text-secondary);
  font-size: 16px;
  font-weight: 500;
}

.page-placeholder p {
  max-width: 360px;
  margin: 0;
  color: var(--app-text-muted);
  font-size: 13px;
  line-height: 1.7;
}

@media (max-width: 1000px) {
  .app-main {
    --app-page-padding-x: 24px;
    --app-page-padding-bottom: 24px;

    padding-top: 27px;
  }
}
</style>
