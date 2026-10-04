<script lang="ts" setup>
import {computed, h, nextTick, onMounted, onUnmounted, ref, watch} from "vue";
import {
  ElButton,
  ElIcon,
  ElInput,
  ElMessage,
  ElMessageBox,
  ElNotification,
  ElOption,
  ElScrollbar,
  ElSelect,
  ElSkeleton,
  ElSkeletonItem,
  ElTooltip,
  type InputInstance,
  type NotificationHandle
} from "element-plus";
import {CloseBold, Download, FolderOpened, RefreshRight} from "@element-plus/icons-vue";
import {invoke, isTauri} from "@tauri-apps/api/core";
import {useI18n} from "vue-i18n";
import PlatformSettingsPanel from "./PlatformSettingsPanel.vue";
import CommandViewer from "./CommandViewer.vue";
import SegmentedToolbar from "./SegmentedToolbar.vue";
import {createVideoParser} from "../composables/useVideoParser";
import {formatVideoSize} from "../composables/videoFormatDisplay";
import {type DownloadCommandOptions} from "../composables/useVideoCommand";
import {platformIds, type VideoPlatform} from "../composables/videoPlatforms";
import {useDesktopActions} from "../composables/useDesktopActions";
import {useCookieSettings} from "../composables/useCookieSettings";
import {usePlatformSettings} from "../composables/usePlatformSettings";
import {videoToolRevision} from "../composables/useRequiredTools";
import {proxySettingsRevision} from "../composables/useProxySettings";
import {useDownloadTasks} from "../composables/useDownloadTasks";
import {useDownloadHistoryActions} from "../composables/downloadHistoryActions";
import {createHistoryFileRecovery} from "../composables/useHistoryFileRecovery";
import DownloadTaskList from "./DownloadTaskList.vue";
import {createDownloadPageState} from "../composables/useDownloadPageState";
import {useUiPreferences} from "../composables/useUiPreferences";
import {useThumbnail} from "../composables/useThumbnail";
import {createHistoryRedownload} from "../composables/useHistoryRedownload";
import type {DownloadRecord} from "../composables/useDownloadHistory";

const props = defineProps<{ active: boolean }>();
const emit = defineEmits<{ busyChange: [busy: boolean]; viewHistory: [id: number] }>();
const {t, te} = useI18n({useScope: "global"});
const cookies = useCookieSettings();
const platformSettings = usePlatformSettings();
const desktop = useDesktopActions();
const pagePersistence = createDownloadPageState({desktop: desktop.desktop, invoke});
const uiPreferences = useUiPreferences();
const initialized = ref(false);
const parser = createVideoParser({
  desktop: isTauri(),
  invoke,
  beforeParse: async selected => {
    if (!await platformSettings.whenIdle(selected)) throw {code: "platformSettingsFailed"};
    return cookies.whenIdle(selected);
  },
  saveCookie: cookies.update,
  onChange: selected => {
    if (initialized.value) void pagePersistence.save(parser.snapshot(selected)).then(saved => {
      if (!saved) ElMessage({type: "error", message: t("persistence.saveFailed"), grouping: true});
    });
  },
  cacheThumbnail: async url => {
    try {
      return await invoke<string>("cache_video_thumbnail", {url});
    } catch {
      ElMessage({type: "error", message: t("persistence.thumbnailFailed"), grouping: true});
      return null;
    }
  },
});
const {
  platform,
  draft,
  video,
  displayVideo,
  displayFormat,
  displayThumbnailCachePath,
  resultLink,
  phase,
  error,
  quality,
  frameRate,
  busy: parserBusy,
  qualities,
  frameRates,
  selectedFormat,
  pasting
} = parser;
const tasks = useDownloadTasks();
const submitting = ref(false);
const busy = computed(() => parserBusy.value || submitting.value || platformSettings.saving[platform.value] || !initialized.value || !uiPreferences.ready.value);
const fileActions = useDownloadHistoryActions();
const redownload = createHistoryRedownload({tasks});
const recovery = createHistoryFileRecovery({
  actions: fileActions, confirmRedownload: async () => {
    try {
      await ElMessageBox.confirm(t('tasks.missingMessage'), t('tasks.missingTitle'), {
        confirmButtonText: t('history.prepare'),
        cancelButtonText: t('history.cancel'),
        type: 'warning'
      });
      return true;
    } catch {
      return false;
    }
  }, redownload: record => redownload.run(record)
});
const coverSource = useThumbnail(displayThumbnailCachePath, computed(() => displayVideo.value?.thumbnail ?? null), () => ElMessage({
  type: "error",
  message: t("persistence.thumbnailReadFailed"),
  grouping: true
}));
const platformOptions = computed(() => platformIds.map(value => ({label: t(`download.platforms.${value}`), value})));
const cookieOpen = ref(false);
const cookieButton = ref<HTMLButtonElement>();
const choosingDirectory = ref(false);
const resettingDirectory = ref(false);
watch(() => busy.value || choosingDirectory.value || resettingDirectory.value, value => emit('busyChange', value), {immediate: true});

function chooseQuality(value: string) {
  parser.setQuality(value);
}

function chooseFrameRate(value: string) {
  parser.setFrameRate(value);
}
const coverFailed = ref(false);
const videoLinkInput = ref<InputInstance>();
let parseFailureNotification: NotificationHandle | undefined;
const downloadOptions = computed<DownloadCommandOptions>(() => {
  const extension = selectedFormat.value?.extension?.toLowerCase();
  return {
    directory: draft.value.directory,
    formatId: selectedFormat.value?.formatId ?? "",
    container: extension === "mp4" ? "mp4" : undefined,
    cookieFallback: video.value?.cookieFallback ?? false,
  };
});
const duration = computed(() => {
  if (displayVideo.value?.duration == null) return "";
  const seconds = Math.floor(displayVideo.value.duration);
  const parts = [Math.floor(seconds / 60) % 60, seconds % 60].map(value => String(value).padStart(2, "0"));
  if (seconds >= 3600) parts.unshift(String(Math.floor(seconds / 3600)));
  return parts.join(":");
});

function messageForError(failure: { code: string } | null, selected: VideoPlatform = platform.value) {
  if (failure?.code === "platformSettingsFailed") return t("download.configuration.loadFailed");
  if (failure?.code === "pageSaveFailed") return t("persistence.saveFailed");
  if (failure?.code === "historyCreateFailed") return t("history.createFailed");
  if (failure && te(`history.${failure.code}`)) return t(`history.${failure.code}`);
  const known = ["invalidLink", "platformMismatch", "desktopOnly", "cookieSaveFailed", "cookieReadFailed", "cookieRequired", "youtubeReloadRequired", "toolMissing", "toolSettingsFailed", "proxySettingsFailed", "spawnFailed", "timeout", "readFailed", "outputTooLarge", "parseFailed", "invalidResult", "unsupportedVideo", "noFormats", "bridgeFailed", "commandUnavailable", "clipboardWriteFailed", "invalidDownloadDirectory", "invalidDownloadOptions", "ffmpegMissing", "denoMissing", "downloadFailed", "downloadResultMissing", "downloadDirectoryFailed", "defaultDirectoryFailed", "downloadTimeout", "downloadBusy", "cancelFailed"];
  const code = failure && known.includes(failure.code) ? failure.code : "bridgeFailed";
  return t(`download.errors.${code}`, {platform: t(`download.platforms.${selected}`)});
}

function failureNotificationMessage(failure: {
  code: string;
  detail?: string
} | null, selected: VideoPlatform = platform.value) {
  const configureTools = failure && ["toolMissing", "ffmpegMissing", "denoMissing", "toolSettingsFailed", "spawnFailed"].includes(failure.code);
  return h("div", [
    h("p", messageForError(failure, selected)),
    failure?.detail ? h("pre", failure.detail) : null,
    configureTools ? h(ElButton, {
      type: "primary", size: "small", class: "tool-settings-action", onClick: openToolSettings,
    }, () => t("download.configureTools")) : null,
  ]);
}

async function startDownload() {
  if (busy.value || choosingDirectory.value || resettingDirectory.value || !selectedFormat.value || !resultLink.value) return;
  closeParseFailureNotification();
  const selected = platform.value;
  submitting.value = true;
  try {
    if (!await platformSettings.whenIdle(selected)) throw {code: 'platformSettingsFailed'};
    if (!await pagePersistence.whenIdle(selected)) throw {code: 'pageSaveFailed'};
    if (!await cookies.whenIdle(selected)) throw {code: 'cookieSaveFailed'};
    const snapshot = JSON.parse(JSON.stringify(parser.snapshot(selected)));
    const request = {snapshot, restoreTrashed: false, redownload: true};
    let result;
    try {
      result = await tasks.submit(request);
    } catch (failure) {
      if ((failure as { code: string })?.code !== 'recordTrashed') throw failure;
      try {
        await ElMessageBox.confirm(t('tasks.restoreMessage'), t('history.restore'), {
          confirmButtonText: t('history.restore'),
          cancelButtonText: t('history.cancel'),
          type: 'warning'
        });
      } catch {
        return;
      }
      result = await tasks.submit({...request, restoreTrashed: true});
    }
    if (result.kind === 'existing') ElMessage.info(t('tasks.alreadyActive'));
  } catch (failure) {
    parseFailureNotification = ElNotification.error({
      title: t('download.transfer.failed'),
      message: failureNotificationMessage(failure as { code: string; detail?: string }),
      duration: 0,
      customClass: 'parse-failure-notification'
    });
  } finally {
    submitting.value = false;
  }
}

async function taskAction(name: string, record: DownloadRecord) {
  if (name === 'cancel') {
    if (!await tasks.cancel(record.requestId)) ElMessage.error(t('download.errors.cancelFailed'));
  } else if (name === 'prepare') await redownload.run(record);
  else if (name === 'details') emit('viewHistory', record.id);
  else if (name === 'openFile') {
    await recovery.openFile(record);
    await tasks.lookupRecord(record.platform, record.videoId);
  } else if (name === 'openFolder') {
    await recovery.openFolder(record);
    await tasks.lookupRecord(record.platform, record.videoId);
  } else if (name === 'copyLink') await fileActions.copyLink(record);
  else if (name === 'openSource') await fileActions.openSource(record);
  if (fileActions.error.value) ElMessage.error(t(`history.${fileActions.error.value.code}`));
}

let restoring = false;

async function restorePageState() {
  if (restoring) return;
  restoring = true;
  try {
    const configurationRevision = `${videoToolRevision.value}:${proxySettingsRevision.value}`;
    initialized.value = false;
    if (!await uiPreferences.load() || !await pagePersistence.load()) return;
    try {
      parser.restore(pagePersistence.states.value);
    } catch {
      pagePersistence.loadError.value = {code: "loadFailed"};
      return;
    }
    parser.selectPlatform(uiPreferences.draft.downloadPlatform);
    initialized.value = true;
    if (configurationRevision !== `${videoToolRevision.value}:${proxySettingsRevision.value}`) parser.resetAll();
  if (!desktop.desktop) return;
  try {
    const directories = await invoke<Record<VideoPlatform, string>>("get_default_download_directories");
    parser.applyDefaultDirectories(directories);
  } catch {
    ElMessage.error(t("download.errors.defaultDirectoryFailed"));
  }
  } finally {
    restoring = false;
  }
}

onMounted(restorePageState);
onMounted(() => {
  void platformSettings.loadProxy();
  platformIds.forEach(selected => {
    void platformSettings.load(selected);
  });
});
watch(uiPreferences.ready, ready => {
  if (ready && !initialized.value) void restorePageState();
});

async function retryPageSave() {
  if (!await pagePersistence.whenIdle(platform.value)) ElMessage.error(t("persistence.saveFailed"));
}

function closeParseFailureNotification() {
  parseFailureNotification?.close();
  parseFailureNotification = undefined;
}

async function parseVideo() {
  if (busy.value) return;
  closeParseFailureNotification();
  const selected = platform.value;
  // Use this attempt's outcome so cached or cancelled errors cannot trigger a notification.
  const failure = await parser.parse();
  if (!failure || parser.error.value !== failure) return;
  parseFailureNotification = ElNotification.error({
    title: t("download.empty.failed"),
    message: failureNotificationMessage(failure, selected),
    position: "top-right",
    duration: 0,
    customClass: "parse-failure-notification",
  });
}

function selectPlatform(value: VideoPlatform) {
  parser.selectPlatform(value);
  if (platform.value === value) void uiPreferences.update({downloadPlatform: value});
}

async function closeCookie() {
  cookieOpen.value = false;
  await nextTick();
  cookieButton.value?.focus();
}

async function updateCookieContents(contents: string) {
  const currentPlatform = platform.value;
  const saved = await parser.updateCookie(contents);
  if (!saved && cookies.saveError[currentPlatform]) {
    ElMessage({type: "error", message: t("download.cookie.saveFailed"), grouping: true});
  }
}

watch(() => props.active, active => {
  if (!active) {
    cookieOpen.value = false;
    parser.cancelPaste();
  }
});
watch(() => [cookieOpen.value, platform.value] as const, async ([open, currentPlatform]) => {
  if (!open) return;
  void platformSettings.load(currentPlatform);
  void platformSettings.loadProxy();
  if (!await cookies.load(currentPlatform)) {
    const key = cookies.saveError[currentPlatform] ? "saveFailed" : "loadFailed";
    ElMessage({type: "error", message: t(`download.cookie.${key}`), grouping: true});
  }
});
watch(cookieOpen, async open => {
  if (open) {
    parser.cancelPaste();
    await nextTick();
    document.getElementById("platform-settings-title")?.focus();
  }
});
watch(displayVideo, () => {
  coverFailed.value = false;
});
watch(videoToolRevision, parser.resetAll);
watch(proxySettingsRevision, parser.resetAll);
watch(proxySettingsRevision, () => {
  void platformSettings.loadProxy();
});

async function openToolSettings() {
  closeParseFailureNotification();
  await navigateToToolSettings();
}

async function navigateToToolSettings() {
  if (!await uiPreferences.update({
    activePage: "settings",
    settingsSection: "tools"
  })) ElMessage.error(t("persistence.saveFailed"));
}

async function openProxySettings() {
  if (!await uiPreferences.update({
    activePage: "settings",
    settingsSection: "proxy"
  })) ElMessage.error(t("persistence.saveFailed"));
}

async function retryPlatformSettings() {
  await Promise.all([platformSettings.load(platform.value), platformSettings.loadProxy()]);
}

async function pasteLink() {
  try {
    if (await parser.pasteLink(desktop.readClipboard)) {
      await nextTick();
      videoLinkInput.value?.focus();
    }
  } catch {
    ElMessage.error(t("download.link.clipboardReadFailed"));
  }
}

async function chooseDirectory() {
  if (busy.value || choosingDirectory.value || resettingDirectory.value || !desktop.desktop) return;
  const selected = platform.value;
  choosingDirectory.value = true;
  try {
    const directory = await desktop.chooseDirectory();
    if (directory !== null && platform.value === selected && !busy.value) parser.setDirectory(directory);
  } catch {
    ElMessage.error(t("download.directoryFailed"));
  } finally {
    choosingDirectory.value = false;
  }
}

async function resetDirectory() {
  if (busy.value || choosingDirectory.value || resettingDirectory.value || !desktop.desktop) return;
  const selected = platform.value;
  resettingDirectory.value = true;
  try {
    const directories = await invoke<Record<VideoPlatform, string>>("get_default_download_directories");
    if (platform.value === selected && !busy.value) parser.resetDirectory(directories[selected]);
  } catch {
    ElMessage.error(t("download.errors.defaultDirectoryFailed"));
  } finally {
    resettingDirectory.value = false;
  }
}

onUnmounted(() => {
  closeParseFailureNotification();
  parser.dispose();
});
</script>

<template>
  <div class="download-page">
    <div class="platform-toolbar">
      <SegmentedToolbar :ariaLabel="t('download.choosePlatform')" :disabled="busy" :model-value="platform"
                        :options="platformOptions" @update:model-value="selectPlatform">
        <template #actions>
          <button ref="cookieButton" :aria-expanded="cookieOpen"
                  :aria-label="t('download.configuration.title', {platform: t(`download.platforms.${platform}`)})"
                  :class="{active: cookieOpen}"
                  :disabled="busy" aria-controls="cookie-inline-panel"
                  class="cookie-trigger"
                  type="button"
                  @click="cookieOpen = !cookieOpen">
            {{ t("download.configuration.configure") }}
          </button>
        </template>
      </SegmentedToolbar>
    </div>

    <div v-if="pagePersistence.loadError.value" class="persistence-error" role="alert">
      {{ t('persistence.loadFailed') }}
      <ElButton size="small" @click="restorePageState">{{ t('persistence.retry') }}</ElButton>
    </div>
    <div v-if="pagePersistence.saveErrors.value[platform]" class="persistence-error" role="alert">
      {{ t('persistence.saveFailed') }}
      <ElButton size="small" @click="retryPageSave">{{ t('persistence.retry') }}</ElButton>
    </div>


    <ElScrollbar :aria-label="t('download.title')" :tabindex="0" class="download-scrollbar"
                 height="100%" role="region" view-class="download-content">
      <div v-if="cookieOpen" id="cookie-inline-panel" @keydown.esc="closeCookie">
        <PlatformSettingsPanel :key="platform" :contents="cookies.contents[platform]"
                               :cookie-disabled="!cookies.desktop || !cookies.ready[platform]"
                               :load-error="platformSettings.loadError[platform] || platformSettings.proxyLoadError.value" :platform="platform"
                               :proxy="platformSettings.proxy.value" :proxy-enabled="platformSettings.settings[platform].proxyEnabled"
                               :save-error="platformSettings.saveError[platform]"
                               :saving="platformSettings.saving[platform]"
                               :settings-disabled="!platformSettings.desktop || !platformSettings.ready[platform] || (!platformSettings.proxyReady.value && !platformSettings.settings[platform].proxyEnabled)"
                               @change="updateCookieContents" @close="closeCookie"
                               @retry="retryPlatformSettings"
                               @proxy-change="platformSettings.updateProxy(platform, $event)" @open-proxy="openProxySettings"/>
      </div>

      <div v-else class="download-card download-workflow">
        <section :aria-label="t('download.link.title')" class="workflow-section link-card">
          <form @submit.prevent="parseVideo">
            <div class="link-row">
              <ElInput id="video-link" ref="videoLinkInput" :aria-invalid="Boolean(error)"
                       :aria-label="t('download.link.title')" :clear-icon="CloseBold" :disabled="busy" :model-value="draft.link"
                       :placeholder="t('download.link.placeholder')"
                       autocomplete="off"
                       clearable
                       @update:model-value="parser.setLink"/>
              <ElTooltip :content="t('download.link.paste')" placement="top">
                <ElButton :aria-label="t('download.link.paste')" :disabled="busy || pasting || !desktop.desktop"
                          :loading="pasting"
                          class="link-paste-button" native-type="button" @click="pasteLink">
                  <ElIcon v-if="!pasting" :size="18" aria-hidden="true">
                    <svg fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"
                         stroke-width="1.7"
                         viewBox="0 0 24 24">
                      <rect height="4" rx="1" width="8" x="8" y="2"/>
                      <path d="M8 4H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h4M16 4h3a2 2 0 0 1 2 2v2"/>
                      <rect height="12" rx="2" width="9" x="12" y="10"/>
                      <path d="M15 14h3m-3 4h3"/>
                    </svg>
                  </ElIcon>
                </ElButton>
              </ElTooltip>
              <ElButton :disabled="busy || pasting || !draft.link.trim()" :loading="phase === 'parsing'"
                        class="parse-link-button" native-type="submit"
                        type="primary">
                {{ t(phase === 'parsing' ? 'download.link.parsing' : 'download.link.parse') }}
              </ElButton>
            </div>
          </form>
        </section>

        <section :aria-label="t('download.result.title')" class="workflow-section result-card">
          <div class="result-layout">
            <div :aria-busy="phase === 'parsing'"
                 :class="{'is-refreshing': Boolean(displayVideo) && phase === 'parsing'}"
                 :inert="Boolean(displayVideo) && phase === 'parsing'" class="result-content">
              <div class="result-media">
                <ElSkeleton v-if="!displayVideo" :animated="phase === 'parsing'" :aria-busy="phase === 'parsing'"
                            :aria-label="t(phase === 'parsing' ? 'download.empty.parsing' : 'download.empty.waitingParse')"
                            class="result-skeleton"
                            role="status">
                  <template #template>
                    <div class="parsed-media">
                      <ElSkeletonItem class="skeleton-cover" variant="rect"/>
                      <div class="skeleton-details">
                        <div class="skeleton-heading">
                          <ElSkeletonItem class="skeleton-title" variant="h1"/>
                          <ElSkeletonItem class="skeleton-title-detail" variant="text"/>
                        </div>
                        <div class="skeleton-meta">
                          <ElSkeletonItem class="skeleton-format" variant="text"/>
                          <ElSkeletonItem class="skeleton-size" variant="text"/>
                        </div>
                      </div>
                    </div>
                  </template>
                </ElSkeleton>
                <div v-else class="parsed-media">
                  <div class="video-cover">
                    <img v-if="coverSource && !coverFailed" :alt="t('download.result.cover')" :src="coverSource"
                         class="video-thumbnail" referrerpolicy="no-referrer" @error="coverFailed = true"/>
                    <span v-else class="cover-unavailable">{{ t('download.result.noCover') }}</span>
                    <span v-if="duration" :aria-label="t('download.result.duration', {duration})"
                          class="video-duration">{{ duration }}</span>
                  </div>
                  <div class="video-details">
                    <h2 id="parsed-video-title">{{ displayVideo.title }}</h2>
                    <div aria-live="polite" class="format-meta">
                      <span>{{
                          t('download.result.format')
                        }} <strong>{{
                          displayFormat?.extension?.toUpperCase() ?? t('download.result.unknown')
                          }}</strong></span>
                      <ElTooltip :content="t('download.result.sizeHint')" placement="top">
                        <span>{{ t('download.result.size') }} <strong>{{
                            formatVideoSize(displayFormat?.sizeBytes) ?? t('download.result.unknown')
                          }}</strong></span>
                      </ElTooltip>
                    </div>
                  </div>
                </div>
              </div>
              <div class="result-options">
                <div class="format-fields">
                  <div>
                    <label class="field-label" for="video-quality">{{ t("download.result.quality") }}</label>
                    <ElSelect id="video-quality" :disabled="busy" :model-value="quality"
                              :no-data-text="t('download.result.noOptions')" placeholder=""
                              @update:model-value="chooseQuality">
                      <ElOption v-for="value in qualities" :key="value" :label="value + 'p'" :value="value"/>
                    </ElSelect>
                  </div>
                  <div>
                    <label class="field-label" for="video-frame-rate">{{ t("download.result.frameRate") }}</label>
                    <ElSelect id="video-frame-rate" :disabled="busy" :model-value="frameRate"
                              :no-data-text="t('download.result.noOptions')" placeholder=""
                              @update:model-value="chooseFrameRate">
                      <ElOption v-for="value in frameRates" :key="value" :label="value + ' fps'" :value="value"/>
                    </ElSelect>
                  </div>
                </div>
              </div>
            </div>
            <div class="parse-command">
              <CommandViewer :active="active" :disabled="!desktop.desktop || !video" :input="resultLink"
                             :platform="platform" :title="t('download.command.title')"/>
            </div>
          </div>
          <p v-if="displayVideo?.cookieFallback" class="format-note" role="status">{{
              t('download.result.cookieFallback')
            }}</p>
        </section>

        <section :aria-label="t('download.title')" class="workflow-section download-dock">
          <header class="progress-heading">
            <div class="download-header-actions">
              <ElButton
                  :disabled="busy || choosingDirectory || resettingDirectory || !desktop.desktop || !selectedFormat || !draft.directory.trim()"
                  :icon="Download"
                  :loading="submitting"
                        class="download-action-button" type="primary" @click="startDownload">
                {{ t('download.actions.start') }}
              </ElButton>

            </div>
            <CommandViewer :active="active" :disabled="!desktop.desktop || !selectedFormat"
                           :download-options="downloadOptions"
                           :input="resultLink" :platform="platform"
                           :title="t('download.command.downloadTitle')"/>
          </header>
          <label class="field-label" for="save-directory">{{ t("download.saveDirectory") }}</label>
          <div class="directory-row">
            <ElInput id="save-directory" :disabled="busy || choosingDirectory || resettingDirectory"
                     :model-value="draft.directory"
                     :placeholder="t('download.directoryPlaceholder')" readonly>
              <template #append>
                <div class="directory-actions">
                  <ElTooltip :content="t('download.chooseDirectory')" placement="top">
                    <ElButton :aria-label="t('download.chooseDirectory')"
                              :disabled="busy || choosingDirectory || resettingDirectory || !desktop.desktop"
                              :icon="FolderOpened"
                              :loading="choosingDirectory" native-type="button" @click="chooseDirectory"/>
                  </ElTooltip>
                  <ElTooltip :content="t('download.resetDirectory')" placement="top">
                    <ElButton :aria-label="t('download.resetDirectory')"
                              :disabled="busy || choosingDirectory || resettingDirectory || !desktop.desktop"
                              :icon="RefreshRight"
                              :loading="resettingDirectory" native-type="button" @click="resetDirectory"/>
                  </ElTooltip>
                </div>
              </template>
            </ElInput>
          </div>

        </section>
      </div>
      <DownloadTaskList :active="active" :busy="fileActions.busy.value" :desktop="desktop.desktop"
                        @action="taskAction"/>
    </ElScrollbar>
  </div>
</template>

<style scoped>
.download-page {
  --download-control-gap: 10px;

  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
}

.download-scrollbar {
  flex: 1;
  min-height: 0;
}

.download-scrollbar :deep(.download-content) {
  padding: 0 var(--app-page-padding-x) var(--app-page-padding-bottom);
}

.platform-toolbar {
  flex-shrink: 0;
  margin: 0 var(--app-page-padding-x) 18px;
}

.cookie-trigger {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 44px;
  padding: 10px 18px;
  border: 1px solid var(--app-download-border);
  border-radius: 9px;
  background: var(--app-download-background);
  color: var(--app-text-secondary);
  font-size: 13px;
  white-space: nowrap;
  cursor: pointer;
}

.cookie-trigger:hover:not(:disabled) {
  background: var(--app-hover);
  color: var(--app-accent);
}

.cookie-trigger.active {
  border-color: transparent;
  background: var(--app-accent-soft);
  color: var(--app-accent);
  font-weight: 600;
}

.cookie-trigger:disabled {
  opacity: .55;
  cursor: not-allowed;
}

.cookie-trigger:focus-visible {
  outline: 2px solid var(--app-accent);
  outline-offset: 2px;
}

.download-workflow {
  display: flex;
  flex-direction: column;
}

.download-card {
  padding: 22px;
  border: 1px solid var(--app-border);
  border-radius: var(--app-radius);
  background: var(--app-surface);
}

.workflow-section + .workflow-section {
  margin-top: 20px;
  padding-top: 20px;
}

.link-row {
  display: flex;
  align-items: stretch;
  gap: 10px;
}

.link-row .el-input {
  flex: 1;
  min-width: 0;
}

.link-row :deep(.el-input__wrapper) {
  min-height: 44px;
  padding: 0 12px;
}

.link-row .el-button {
  height: 44px;
  margin-left: 0;
}

.parse-link-button {
  min-width: 104px;
}

.link-paste-button {
  width: 44px;
  flex-shrink: 0;
  padding: 0;
  color: var(--app-text-secondary);
}

.link-paste-button:focus-visible {
  outline: 2px solid var(--app-accent);
  outline-offset: 1px;
}

:global(.parse-failure-notification) {
  max-width: calc(100vw - 32px);
}

:global(.parse-failure-notification p) {
  margin: 0;
}

:global(.parse-failure-notification .tool-settings-action) {
  margin-top: 12px;
}

:global(.parse-failure-notification pre) {
  margin: 6px 0 0;
  max-height: 180px;
  overflow: auto;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  font: inherit;
}

.result-layout {
  position: relative;
}

.parse-command {
  position: absolute;
  top: 0;
  right: 0;
  display: flex;
}

.result-content {
  display: grid;
  gap: 20px;
}

.result-content.is-refreshing {
  opacity: .55;
}

.result-media {
  min-height: 88px;
  padding-right: 44px;
}

.parsed-media {
  display: flex;
  align-items: center;
  gap: 16px;
}

.video-cover {
  position: relative;
  width: 156px;
  flex-shrink: 0;
  aspect-ratio: 16 / 9;
  overflow: hidden;
  border: 1px solid var(--app-border);
  border-radius: 8px;
  background: var(--app-background);
}

.video-thumbnail {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.cover-unavailable {
  display: grid;
  height: 100%;
  place-items: center;
  color: var(--app-text-secondary);
  font-size: 11px;
}

.video-duration {
  position: absolute;
  right: 6px;
  bottom: 6px;
  padding: 2px 5px;
  border-radius: 4px;
  background: #23372bba;
  color: white;
  font-size: 10px;
}

.video-details,
.skeleton-details {
  flex: 1;
  align-self: stretch;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  gap: 8px;
  min-width: 0;
}

.video-details h2 {
  margin: 0;
  font-size: 17px;
  font-weight: 600;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

.result-skeleton {
  min-height: 88px;
}

.skeleton-cover {
  width: 156px;
  height: auto;
  flex-shrink: 0;
  aspect-ratio: 16 / 9;
  border-radius: 8px;
}

.skeleton-heading {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.skeleton-title {
  width: 88%;
  height: 18px;
}

.skeleton-title-detail {
  width: 62%;
  height: 16px;
}

.skeleton-meta {
  min-height: 20px;
  align-items: center;
}

.skeleton-format {
  width: 88px;
  height: 14px;
}

.skeleton-size {
  width: 112px;
  height: 14px;
}

.format-fields {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--download-control-gap);
}

.format-fields > div {
  min-width: 0;
}

.field-label {
  display: block;
  margin-bottom: 6px;
  color: var(--app-text-secondary);
  font-size: 12px;
}

.format-fields .el-select {
  width: 100%;
}

.result-options {
  display: grid;
  gap: 10px;
}

.format-meta,
.skeleton-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 6px 20px;
}

.format-meta {
  color: var(--app-text-secondary);
  font-size: 12px;
  line-height: 1.7;
}

.format-meta strong {
  margin-left: 4px;
  color: var(--app-text);
  font-weight: 500;
}

.format-note {
  margin: 12px 0 0;
  color: var(--app-text-secondary);
  font-size: 11px;
  line-height: 1.7;
}

.progress-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 16px;
}

.download-header-actions {
  display: grid;
  grid-auto-flow: column;
  grid-auto-columns: minmax(120px, 1fr);
  align-items: center;
  gap: var(--download-control-gap);
}

.directory-row {
  display: flex;
}

.directory-row .el-input {
  flex: 1;
  min-width: 0;
}

.directory-row :deep(.el-input__wrapper) {
  min-height: 36px;
  padding: 0 12px;
}

.directory-row :deep(.el-input-group__append) {
  padding: 0;
  background: var(--app-surface);
}

.directory-actions {
  display: flex;
  align-items: stretch;
}

.directory-row :deep(.el-input-group__append .el-button) {
  width: 40px;
  height: 36px;
  margin: 0;
  padding: 0;
  border: 0;
  border-radius: 0;
}

.directory-actions > .el-button + .el-button {
  border-left: 1px solid var(--app-border);
}

.directory-actions > .el-button:last-child {
  border-top-right-radius: var(--el-border-radius-base);
  border-bottom-right-radius: var(--el-border-radius-base);
}

.directory-actions > .el-button:hover:not(:disabled) {
  background: var(--app-accent-soft);
  color: var(--app-accent);
}

.directory-actions > .el-button:focus-visible {
  outline: 2px solid var(--app-accent);
  outline-offset: -3px;
}

.download-header-actions > .download-action-button {
  width: 100%;
  min-width: 120px;
  height: 36px;
  margin: 0;
  border-radius: var(--el-border-radius-base);
}

.download-progress {
  display: grid;
  gap: 15px;
  margin-top: 16px;
}

.download-progress .field-label {
  margin-bottom: 0;
}

.download-progress-inline :deep(.el-progress__text) {
  min-width: 50px;
  text-align: right;
}

.download-speed-heading {
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.progress-percentage,
.progress-speed {
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
}

.progress-speed {
  color: var(--app-text-secondary);
  font-size: 12px;
}

@media (max-width: 1000px) {
  .cookie-trigger {
    padding: 10px 14px;
  }
}

@media (max-width: 800px) {
  .link-row {
    flex-wrap: wrap;
  }

  .link-row .el-input {
    flex-basis: 100%;
  }

  .link-paste-button {
    margin-left: auto !important;
  }

  .download-card {
    padding: 18px;
  }

  .cookie-trigger {
    padding: 10px 12px;
  }

  .format-fields {
    grid-template-columns: 1fr;
  }

  .parsed-media {
    align-items: flex-start;
    gap: 12px;
  }

  .video-cover, .skeleton-cover {
    width: 120px;
  }

  .video-details h2 {
    font-size: 15px;
  }

  .progress-heading {
    flex-wrap: wrap;
  }
}
</style>
