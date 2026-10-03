<script lang="ts" setup>
import {computed, h, nextTick, onMounted, onUnmounted, ref, watch} from "vue";
import {
  ElButton,
  ElIcon,
  ElInput,
  ElMessage,
  ElNotification,
  ElOption,
  ElProgress,
  ElScrollbar,
  ElSelect,
  ElSkeleton,
  ElSkeletonItem,
  ElTooltip,
  type InputInstance,
  type NotificationHandle
} from "element-plus";
import {CloseBold, Download, FolderOpened} from "@element-plus/icons-vue";
import {Channel, invoke, isTauri} from "@tauri-apps/api/core";
import {useI18n} from "vue-i18n";
import CookieImportPanel from "./CookieImportPanel.vue";
import CommandViewer from "./CommandViewer.vue";
import SegmentedToolbar from "./SegmentedToolbar.vue";
import {createVideoParser} from "../composables/useVideoParser";
import {formatVideoSize} from "../composables/videoFormatDisplay";
import {type DownloadCommandOptions} from "../composables/useVideoCommand";
import {platformIds, type VideoPlatform} from "../composables/videoPlatforms";
import {useDesktopActions} from "../composables/useDesktopActions";
import {useCookieSettings} from "../composables/useCookieSettings";
import {videoToolRevision} from "../composables/useRequiredTools";
import {createVideoDownload, type DownloadProgress} from "../composables/useVideoDownload";

const props = defineProps<{ active: boolean }>();
const {t} = useI18n({useScope: "global"});
const cookies = useCookieSettings();
const desktop = useDesktopActions();
const parser = createVideoParser({
  desktop: isTauri(),
  invoke,
  beforeParse: cookies.whenIdle,
  saveCookie: cookies.update
});
const {
  platform,
  draft,
  video,
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
const downloader = createVideoDownload({
  desktop: desktop.desktop,
  invoke,
  beforeDownload: cookies.whenIdle,
  createChannel: handler => {
    const channel = new Channel<DownloadProgress>();
    channel.onmessage = handler;
    return channel;
  },
});
const busy = computed(() => parserBusy.value || downloader.busy.value);
const platformOptions = computed(() => platformIds.map(value => ({label: t(`download.platforms.${value}`), value})));
const downloadPhase = downloader.phase;
const downloadProgress = downloader.progress;
const downloadError = downloader.error;
const progressPercentage = computed(() => Math.round(downloader.percentage.value * 10) / 10);
const downloadSpeed = computed(() => {
  const speed = downloadPhase.value === "downloading" ? downloadProgress.value?.speed : null;
  if (speed == null || !Number.isFinite(speed) || speed < 0) return "—";
  if (speed === 0) return "0 B/s";
  const size = formatVideoSize(Math.round(speed));
  return size ? `${size}/s` : "—";
});
const progressStatus = computed(() => {
  if (downloadPhase.value === "completed") return "success";
  if (downloadPhase.value === "failed") return "exception";
  if (downloadPhase.value === "cancelled") return "warning";
  return undefined;
});
const cookieOpen = ref(false);
const cookieButton = ref<HTMLButtonElement>();
const choosingDirectory = ref(false);
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
  if (video.value?.duration == null) return "";
  const seconds = Math.floor(video.value.duration);
  const parts = [Math.floor(seconds / 60) % 60, seconds % 60].map(value => String(value).padStart(2, "0"));
  if (seconds >= 3600) parts.unshift(String(Math.floor(seconds / 3600)));
  return parts.join(":");
});

function messageForError(failure: { code: string } | null, selected: VideoPlatform = platform.value) {
  const known = ["invalidLink", "platformMismatch", "desktopOnly", "cookieSaveFailed", "cookieReadFailed", "cookieRequired", "youtubeReloadRequired", "toolMissing", "toolSettingsFailed", "spawnFailed", "timeout", "readFailed", "outputTooLarge", "parseFailed", "invalidResult", "unsupportedVideo", "noFormats", "bridgeFailed", "commandUnavailable", "clipboardWriteFailed", "invalidDownloadDirectory", "invalidDownloadOptions", "ffmpegMissing", "downloadFailed", "downloadResultMissing", "downloadDirectoryFailed", "defaultDirectoryFailed", "downloadTimeout", "downloadBusy", "cancelFailed"];
  const code = failure && known.includes(failure.code) ? failure.code : "bridgeFailed";
  return t(`download.errors.${code}`, {platform: t(`download.platforms.${selected}`)});
}

async function startDownload() {
  if (busy.value || choosingDirectory.value || !selectedFormat.value || !resultLink.value) return;
  closeParseFailureNotification();
  await downloader.start({platform: platform.value, input: resultLink.value, options: downloadOptions.value});
  if (downloadPhase.value === "failed") {
    parseFailureNotification = ElNotification.error({
      title: t("download.transfer.failed"),
      message: h("div", [h("p", messageForError(downloadError.value)), downloadError.value?.detail ? h("pre", downloadError.value.detail) : null]),
      duration: 0, customClass: "parse-failure-notification"
    });
  } else if (downloadPhase.value === "completed" && downloader.result.value?.alreadyDownloaded) {
    ElMessage.info(t("download.transfer.alreadyDownloaded"));
  }
}

async function cancelDownload() {
  if (!await downloader.cancel()) ElMessage.error(t("download.errors.cancelFailed"));
}

onMounted(async () => {
  if (!desktop.desktop) return;
  try {
    const directories = await invoke<Record<VideoPlatform, string>>("get_default_download_directories");
    parser.applyDefaultDirectories(directories);
  } catch {
    ElMessage.error(t("download.errors.defaultDirectoryFailed"));
  }
});
watch(() => [platform.value, resultLink.value, selectedFormat.value?.formatId, selectedFormat.value?.extension, draft.value.directory], downloader.reset);

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
    message: h("div", [
      h("p", messageForError(failure, selected)),
      failure.detail ? h("pre", failure.detail) : null,
    ]),
    position: "top-right",
    duration: 0,
    customClass: "parse-failure-notification",
  });
}

function selectPlatform(value: VideoPlatform) {
  parser.selectPlatform(value);
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
  if (open && !await cookies.load(currentPlatform)) {
    const key = cookies.saveError[currentPlatform] ? "saveFailed" : "loadFailed";
    ElMessage({type: "error", message: t(`download.cookie.${key}`), grouping: true});
  }
});
watch(cookieOpen, async open => {
  if (open) {
    parser.cancelPaste();
    await nextTick();
    document.getElementById("cookie-panel-title")?.focus();
  }
});
watch(video, () => {
  coverFailed.value = false;
});
watch(videoToolRevision, parser.resetAll);

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
  const selected = platform.value;
  choosingDirectory.value = true;
  try {
    const directory = await desktop.chooseDirectory();
    if (directory !== null && platform.value === selected) parser.setDirectory(directory);
  } catch {
    ElMessage.error(t("download.directoryFailed"));
  } finally {
    choosingDirectory.value = false;
  }
}

onUnmounted(() => {
  closeParseFailureNotification();
  parser.dispose();
  downloader.dispose();
});
</script>

<template>
  <div class="download-page">
    <div class="platform-toolbar">
      <SegmentedToolbar :ariaLabel="t('download.choosePlatform')" :disabled="busy" :model-value="platform"
                        :options="platformOptions" @update:model-value="selectPlatform">
        <template #actions>
          <button ref="cookieButton" :aria-expanded="cookieOpen"
                  :aria-label="t('download.manageCookie', {platform: t(`download.platforms.${platform}`)})"
                  :class="{active: cookieOpen}"
                  :disabled="busy" aria-controls="cookie-inline-panel"
                  class="cookie-trigger"
                  type="button"
                  @click="cookieOpen = !cookieOpen">
            {{ t("download.cookie.configure") }}
          </button>
        </template>
      </SegmentedToolbar>
    </div>

    <ElScrollbar :aria-label="t('download.title')" :tabindex="0" class="download-scrollbar"
                 height="100%" role="region" view-class="download-content">
      <div v-if="cookieOpen" id="cookie-inline-panel" @keydown.esc="closeCookie">
        <CookieImportPanel :key="platform" :contents="cookies.contents[platform]"
                           :disabled="!cookies.desktop || !cookies.ready[platform]"
                           :platform="platform"
                           @change="updateCookieContents" @close="closeCookie"/>
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
            <div :aria-busy="phase === 'parsing'" :class="{'is-refreshing': Boolean(video) && phase === 'parsing'}"
                 :inert="Boolean(video) && phase === 'parsing'" class="result-content">
              <div class="result-media">
                <ElSkeleton v-if="!video" :animated="phase === 'parsing'" :aria-busy="phase === 'parsing'"
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
                    <img v-if="video.thumbnail && !coverFailed" :alt="t('download.result.cover')" :src="video.thumbnail"
                         class="video-thumbnail" referrerpolicy="no-referrer" @error="coverFailed = true"/>
                    <span v-else class="cover-unavailable">{{ t('download.result.noCover') }}</span>
                    <span v-if="duration" :aria-label="t('download.result.duration', {duration})"
                          class="video-duration">{{ duration }}</span>
                  </div>
                  <div class="video-details">
                    <h2 id="parsed-video-title">{{ video.title }}</h2>
                    <div aria-live="polite" class="format-meta">
                      <span>{{
                          t('download.result.format')
                        }} <strong>{{
                            selectedFormat?.extension?.toUpperCase() ?? t('download.result.unknown')
                          }}</strong></span>
                      <ElTooltip :content="t('download.result.sizeHint')" placement="top">
                        <span>{{ t('download.result.size') }} <strong><span v-if="selectedFormat?.sizeApproximate">{{
                            t('download.result.approximate')
                          }}</span>{{
                            formatVideoSize(selectedFormat?.sizeBytes) ?? t('download.result.unknown')
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
                              @update:model-value="parser.setQuality">
                      <ElOption v-for="value in qualities" :key="value" :label="value + 'p'" :value="value"/>
                    </ElSelect>
                  </div>
                  <div>
                    <label class="field-label" for="video-frame-rate">{{ t("download.result.frameRate") }}</label>
                    <ElSelect id="video-frame-rate" :disabled="busy" :model-value="frameRate"
                              :no-data-text="t('download.result.noOptions')" placeholder=""
                              @update:model-value="parser.setFrameRate">
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
          <p v-if="video?.cookieFallback" class="format-note" role="status">{{
              t('download.result.cookieFallback')
            }}</p>
        </section>

        <section :aria-label="t('download.title')" class="workflow-section download-dock">
          <header class="progress-heading">
            <div class="download-header-actions">
              <ElButton :disabled="busy || choosingDirectory || !desktop.desktop || !selectedFormat || !draft.directory.trim()" :icon="Download"
                        :loading="downloader.busy.value"
                        class="download-action-button" type="primary" @click="startDownload">
                {{ t('download.actions.start') }}
              </ElButton>
              <ElButton v-if="downloader.busy.value" :disabled="downloadPhase === 'cancelling'"
                        class="download-action-button" @click="cancelDownload">
                {{ t('download.actions.cancel') }}
              </ElButton>
            </div>
            <CommandViewer :active="active" :disabled="!desktop.desktop || !selectedFormat"
                           :download-options="downloadOptions"
                           :input="resultLink" :platform="platform"
                           :title="t('download.command.downloadTitle')"/>
          </header>
          <label class="field-label" for="save-directory">{{ t("download.saveDirectory") }}</label>
          <div class="directory-row">
            <ElInput id="save-directory" :disabled="busy" :model-value="draft.directory"
                     :placeholder="t('download.directoryPlaceholder')"
                     @update:model-value="parser.setDirectory">
              <template #prefix>
                <ElIcon :size="17" aria-hidden="true">
                  <FolderOpened/>
                </ElIcon>
              </template>
            </ElInput>
            <ElButton :disabled="busy || choosingDirectory || !desktop.desktop" :icon="FolderOpened"
                      :loading="choosingDirectory" @click="chooseDirectory">{{ t("download.chooseDirectory") }}
            </ElButton>
          </div>
          <div class="download-progress">
            <div class="download-speed-heading">
              <span class="field-label">{{ t('download.transfer.speed') }}</span>
              <span class="progress-speed">{{ downloadSpeed }}</span>
            </div>
            <ElProgress :aria-label="t('download.transfer.stream')" :percentage="progressPercentage" :status="progressStatus"
                        :stroke-width="6" class="download-progress-inline">
              <template #default>
                <span class="progress-percentage">{{ progressPercentage }}%</span>
              </template>
            </ElProgress>
          </div>
        </section>
      </div>
    </ElScrollbar>
  </div>
</template>

<style scoped>
.download-page {
  --app-text-secondary: #617568;
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
  border: 1px solid #2e6b5012;
  border-radius: 9px;
  background: #2e6b5006;
  color: var(--app-text-secondary);
  font-size: 13px;
  white-space: nowrap;
  cursor: pointer;
}

.cookie-trigger:hover:not(:disabled) {
  background: #f0f5f1;
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
  gap: var(--download-control-gap);
}

.directory-row .el-input {
  flex: 1;
  min-width: 0;
}

.directory-row :deep(.el-input__wrapper) {
  min-height: 36px;
  padding: 0 12px;
}

.directory-row > .el-button {
  height: 36px;
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
