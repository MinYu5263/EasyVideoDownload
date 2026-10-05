<script setup lang="ts">
import {computed, nextTick, onMounted, onUnmounted, ref, watch} from "vue";
import {ElConfigProvider} from "element-plus";
import {useI18n} from "vue-i18n";
import AppSidebar from "./components/AppSidebar.vue";
import DownloadPage from "./components/DownloadPage.vue";
import SettingsPage from "./components/SettingsPage.vue";
import HistoryPage from "./components/HistoryPage.vue";
import ContentMotion from "./components/ContentMotion.vue";
import {createPageNavigation} from "./composables/usePageNavigation";
import CloseWindowDialog from "./components/CloseWindowDialog.vue";
import {elementPlusLocale} from "./i18n";
import {appPages} from "./navigation";
import {useUiPreferences} from "./composables/useUiPreferences";
import {useAppSettings} from "./composables/useAppSettings";
import {useAppAppearance} from "./composables/useAppAppearance";
import {listen, type UnlistenFn} from "@tauri-apps/api/event";
import {useDownloadHistory} from "./composables/useDownloadHistory";
import {useDownloadTasks} from "./composables/useDownloadTasks";
import {useFeedback} from "./composables/useFeedback";
import {usePlatformDirectories} from "./composables/usePlatformDirectories";
import {platformIds} from "./composables/videoPlatforms";

const uiPreferences = useUiPreferences();
const appearance = useAppAppearance();
const appSettings = useAppSettings();
const nativeError = ref<{ code: string; detail: string } | null>(null);
let unlistenNativeError: UnlistenFn | undefined;
let unlistenDownloadNotice: UnlistenFn | undefined;
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
  try {
    const unlisten = await listen<{
      requestId: string;
      notice: { title: string; body: string; kind: "success" | "error"; detail: string | null };
    }>("app-download-notice", ({payload}) => {
      if (disposed) return;
      const options = {detail: payload.notice.detail ?? undefined, key: `download-outcome:${payload.requestId}`};
      if (payload.notice.kind === 'error') notifyDownloadFailure(t('download.errors.downloadFailed'), options);
      else notify(payload.notice.body, 'success', {...options, downloadOutcome: 'completed'});
    });
    if (disposed) unlisten();
    else unlistenDownloadNotice = unlisten;
  } catch (error) {
    nativeError.value = {code: "downloadNoticeListenFailed", detail: String(error)};
  }
  if (!disposed) await tasks.connect();
});
onUnmounted(() => {
  disposed = true;
  unlistenNativeError?.();
  unlistenDownloadNotice?.();
});

const tasks = useDownloadTasks();
onUnmounted(tasks.dispose);
const historyPage = ref<InstanceType<typeof HistoryPage>>();
const pageMotion = ref<InstanceType<typeof ContentMotion>>();
let historyNavigation = 0;

async function viewHistory(id: number, trashed = false) {
  activePageId.value = "history";
  const navigation = ++historyNavigation;
  await nextTick();
  await pageMotion.value?.whenIdle();
  if (navigation !== historyNavigation || activePageId.value !== 'history') return;
  await historyPage.value?.revealRecord(id, trashed);
}

const history = useDownloadHistory();
const downloadBusy = ref(true);
onMounted(history.refresh);
onUnmounted(history.dispose);
const {activePageId} = createPageNavigation(uiPreferences);
watch(activePageId, () => {
  historyNavigation++;
}, {flush: 'sync'});

const {t} = useI18n({useScope: "global"});
const {watchError, notifyError, notify, notifyDownloadFailure} = useFeedback();
watchError(uiPreferences.loadError, () => t('persistence.loadFailed'), () => ({key: 'preferences:load'}));
watchError(uiPreferences.saveError, () => t('persistence.saveFailed'), () => ({key: 'preferences:save'}));
watchError(tasks.error, error => t(error.code === 'cancelFailed' ? 'download.errors.cancelFailed' :
    ['pauseFailed', 'resumeFailed'].includes(error.code) ? `tasks.${error.code}` : 'tasks.connectionFailed'), error => ({
  key: 'tasks:service', downloadOutcome: error.code === 'resumeFailed' ? 'failed' : undefined,
}));
watchError(history.connectionError, () => t('history.listenFailed'), () => ({key: 'history:connection'}));
watchError(() => appearance.error.value ? {
  code: 'applyFailed',
  detail: appearance.error.value
} : null, () => t('settings.theme.applyFailed'), () => ({key: 'appearance'}));
watchError(nativeError, error => t(`settings.nativeErrors.${error.code}`), () => ({key: 'native'}));
watchError(appSettings.loadError, error => t(`settings.persistence.errors.${error.code}`), () => ({key: 'app-settings:load'}));
watchError(appSettings.saveError, error => t(`settings.persistence.errors.${error.code}`), () => ({key: 'app-settings:save'}));
const directories = usePlatformDirectories();
for (const platform of platformIds) watchError(() => directories.error[platform], error => t(error.code === 'invalidDownloadDirectory' ? 'settings.platforms.invalidDirectory' : 'settings.platforms.directoryFailed'), () => ({
  key: `directory:${platform}`,
  title: t('download.configuration.title', {platform: t(`download.platforms.${platform}`)}),
}));
watch(tasks.tasks, (current, previous = {}) => {
  for (const [id, task] of Object.entries(current)) if (task.storageError &&
      JSON.stringify(task.storageError) !== JSON.stringify(previous[Number(id)]?.storageError)) {
    notifyError(t('history.saveFailed'), {
      key: `task-storage:${id}`,
      title: task.record.title,
      detail: task.storageError.detail
    });
  }
}, {immediate: true});
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

      <main id="main-content" :aria-label="t(activePage.labelKey)" class="app-main" tabindex="-1">
        <ContentMotion ref="pageMotion" :position="appPages.findIndex(page => page.id === activePage.id)"
                       :view-key="activePage.id" axis="vertical">
        <DownloadPage v-show="activePage.id === 'download'" :active="activePage.id === 'download'"
                      @busy-change="downloadBusy = $event" @view-history="viewHistory"/>
        <SettingsPage v-show="activePage.id === 'settings'"/>
        <HistoryPage v-show="activePage.id === 'history'" ref="historyPage" :active="activePage.id === 'history'"
                     :downloading="downloadBusy"/>
        </ContentMotion>
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
  padding-top: 16px;
}

.app-main:focus {
  outline: none;
}

.page-scrollbar {
  flex: 1;
  min-height: 0;
}

.page-scrollbar :deep(.page-content) {
  padding: 0 var(--app-page-padding-x) var(--app-page-padding-bottom);
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

    padding-top: 16px;
  }
}
</style>
