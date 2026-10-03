<script lang="ts" setup>
import {computed, h, nextTick, onUnmounted, ref, watch} from "vue";
import {
  ElButton,
  ElIcon,
  ElInput,
  ElMessage,
  ElNotification,
  ElOption,
  ElSelect,
  ElSkeleton,
  ElSkeletonItem,
  ElTooltip,
  type InputInstance,
  type NotificationHandle
} from "element-plus";
import {Download, FolderOpened} from "@element-plus/icons-vue";
import {invoke, isTauri} from "@tauri-apps/api/core";
import {useI18n} from "vue-i18n";
import CookieImportPanel from "./CookieImportPanel.vue";
import CommandViewer from "./CommandViewer.vue";
import {createVideoParser} from "../composables/useVideoParser";
import {formatVideoSize} from "../composables/videoFormatDisplay";
import {type DownloadCommandOptions} from "../composables/useVideoCommand";
import {platformIds, type VideoPlatform} from "../composables/videoPlatforms";
import {useDesktopActions} from "../composables/useDesktopActions";
import {useCookieSettings} from "../composables/useCookieSettings";
import {videoToolRevision} from "../composables/useRequiredTools";

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
  busy,
  qualities,
  frameRates,
  selectedFormat,
  pasting
} = parser;
const cookieOpen = ref(false);
const cookieButton = ref<HTMLButtonElement>();
const choosingDirectory = ref(false);
const coverFailed = ref(false);
const videoLinkInput = ref<InputInstance>();
let parseFailureNotification: NotificationHandle | undefined;
const selection = computed(() => [quality.value ? quality.value + "p" : null,
  frameRate.value ? frameRate.value + " fps" : null].filter(Boolean).join(" · "));
const downloadOptions = computed<DownloadCommandOptions>(() => ({
  directory: draft.value.directory,
  formatId: selectedFormat.value?.formatId ?? ""
}));
const duration = computed(() => {
  if (video.value?.duration == null) return "";
  const seconds = Math.floor(video.value.duration);
  const parts = [Math.floor(seconds / 60) % 60, seconds % 60].map(value => String(value).padStart(2, "0"));
  if (seconds >= 3600) parts.unshift(String(Math.floor(seconds / 3600)));
  return parts.join(":");
});

function messageForError(failure: { code: string } | null, selected: VideoPlatform = platform.value) {
  const known = ["invalidLink", "platformMismatch", "desktopOnly", "cookieSaveFailed", "cookieReadFailed", "cookieRequired", "youtubeReloadRequired", "toolMissing", "toolSettingsFailed", "spawnFailed", "timeout", "readFailed", "outputTooLarge", "parseFailed", "invalidResult", "unsupportedVideo", "noFormats", "bridgeFailed", "commandUnavailable", "clipboardWriteFailed"];
  const code = failure && known.includes(failure.code) ? failure.code : "bridgeFailed";
  return t(`download.errors.${code}`, {platform: t(`download.platforms.${selected}`)});
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
});
</script>

<template>
  <div class="download-page">
    <div class="platform-toolbar">
      <div :aria-label="t('download.choosePlatform')" class="platform-choices" role="group">
        <button v-for="id in platformIds" :key="id" :aria-pressed="platform === id" :class="{active: platform === id}"
                :disabled="busy"
                class="platform-button" type="button" @click="selectPlatform(id)">
          {{ t(`download.platforms.${id}`) }}
        </button>
      </div>
      <div class="cookie-toolbar-action">
        <button ref="cookieButton" :aria-expanded="cookieOpen" :aria-label="t('download.manageCookie', {platform: t(`download.platforms.${platform}`)})" :class="{active: cookieOpen}"
                :disabled="busy" aria-controls="cookie-inline-panel"
                class="platform-button cookie-trigger"
                type="button"
                @click="cookieOpen = !cookieOpen">
          {{ t("download.cookie.configure") }}
        </button>
      </div>
    </div>

    <div v-if="cookieOpen" id="cookie-inline-panel" @keydown.esc="closeCookie">
      <CookieImportPanel :key="platform" :contents="cookies.contents[platform]" :disabled="!cookies.desktop || !cookies.ready[platform]"
                         :platform="platform"
                         :save-failed="Boolean(cookies.saveError[platform])"
                         @change="updateCookieContents" @close="closeCookie"/>
    </div>

    <div v-else class="download-workflow">
      <section aria-labelledby="video-link-title" class="download-card link-card">
        <form @submit.prevent="parseVideo">
          <div class="card-heading">
            <label id="video-link-title" class="card-label" for="video-link">{{ t("download.link.title") }}</label>
          </div>
          <div class="link-row">
            <ElInput id="video-link" ref="videoLinkInput" :aria-invalid="Boolean(error)" :disabled="busy" :model-value="draft.link"
                     :placeholder="t('download.link.placeholder')"
                     autocomplete="off"
                     clearable
                     @update:model-value="parser.setLink"/>
            <ElTooltip :content="t('download.link.paste')" placement="top">
              <ElButton :aria-label="t('download.link.paste')" :disabled="busy || pasting || !desktop.desktop" :loading="pasting"
                        class="link-paste-button" native-type="button" @click="pasteLink">
                <ElIcon v-if="!pasting" :size="18" aria-hidden="true">
                  <svg fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.7"
                       viewBox="0 0 24 24">
                    <rect height="4" rx="1" width="8" x="8" y="2"/>
                    <path d="M8 4H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h4M16 4h3a2 2 0 0 1 2 2v2"/>
                    <rect height="12" rx="2" width="9" x="12" y="10"/>
                    <path d="M15 14h3m-3 4h3"/>
                  </svg>
                </ElIcon>
              </ElButton>
            </ElTooltip>
            <ElButton :disabled="busy || pasting || !draft.link.trim()" :loading="phase === 'parsing'" class="parse-link-button" native-type="submit"
                      type="primary">
              {{ t(phase === 'parsing' ? 'download.link.parsing' : 'download.link.parse') }}
            </ElButton>
          </div>
        </form>
      </section>

      <section aria-labelledby="video-result-title" class="download-card result-card">
        <header class="parsed-summary">
          <h2 id="video-result-title" class="parsed-state">{{ t('download.result.title') }}</h2>
          <CommandViewer :active="active" :disabled="!desktop.desktop || !video" :input="resultLink"
                         :platform="platform" :title="t('download.command.title')"/>
        </header>
        <div :aria-busy="phase === 'parsing'" :class="{'is-refreshing': Boolean(video) && phase === 'parsing'}"
             :inert="Boolean(video) && phase === 'parsing'" class="result-content">
          <div class="result-media">
            <ElSkeleton v-if="!video" :animated="phase === 'parsing'" :aria-busy="phase === 'parsing'"
                        :aria-label="t(phase === 'parsing' ? 'download.empty.parsing' : 'download.empty.waitingParse')" class="result-skeleton"
                        role="status">
              <template #template>
                <div class="parsed-media">
                  <ElSkeletonItem class="skeleton-cover" variant="rect"/>
                  <div class="skeleton-details">
                    <ElSkeletonItem class="skeleton-title" variant="h1"/>
                    <ElSkeletonItem class="skeleton-title-detail" variant="text"/>
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
            <div v-if="video" aria-live="polite" class="format-meta">
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
        <p v-if="video?.cookieFallback" class="format-note" role="status">{{ t('download.result.cookieFallback') }}</p>
      </section>

      <section aria-labelledby="download-status-title" class="download-card download-dock">
        <header class="progress-heading">
          <h2 id="download-status-title">{{ t(`download.status.${phase}`) }}</h2>
          <CommandViewer :active="active" :disabled="!desktop.desktop || !selectedFormat" :download-options="downloadOptions"
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
        <div class="dock-actions">
          <div class="dock-status" role="status">
            <p v-if="phase === 'ready'">{{ selection }}</p>
            <p>{{ t('download.downloadPending') }}</p>
          </div>
          <ElButton :icon="Download" class="download-button" disabled type="primary">
            {{ t('download.actions.start') }}
          </ElButton>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.download-page {
  --app-text-secondary: #617568;
}

.platform-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 18px;
  padding: 8px;
  border: 1px solid var(--app-border);
  border-radius: var(--app-radius);
  background: var(--app-surface);
}

.platform-choices {
  display: flex;
  align-items: center;
  gap: 4px;
}

.platform-button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 44px;
  padding: 10px 18px;
  border: 0;
  border-radius: 9px;
  background: transparent;
  color: var(--app-text-secondary);
  font-size: 13px;
  white-space: nowrap;
  cursor: pointer;
}

.platform-button:hover:not(:disabled) {
  background: #f0f5f1;
  color: var(--app-accent);
}

.platform-button.active {
  background: var(--app-accent-soft);
  color: var(--app-accent);
  font-weight: 600;
}

.platform-button:disabled {
  opacity: .55;
  cursor: not-allowed;
}

.platform-button:focus-visible {
  outline: 2px solid var(--app-accent);
  outline-offset: 2px;
}

.cookie-toolbar-action {
  display: flex;
  align-items: center;
  margin-left: auto;
  padding-left: 12px;
  border-left: 1px solid var(--app-border);
}

.cookie-trigger {
  border: 1px solid #2e6b5012;
  background: #2e6b5006;
}

.cookie-trigger.active {
  border-color: transparent;
}

.download-workflow {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.download-card {
  padding: 22px;
  border: 1px solid var(--app-border);
  border-radius: var(--app-radius);
  background: var(--app-surface);
}

.card-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 12px;
}

.card-label {
  display: block;
  font-size: 13px;
  font-weight: 600;
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

.parsed-summary {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 16px;
}

.parsed-state {
  margin: 0;
  color: var(--app-text-secondary);
  font-size: 13px;
  font-weight: 600;
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

.video-details {
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

.skeleton-details {
  flex: 1;
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

.format-fields {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px;
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

.format-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 6px 20px;
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

.download-dock {
  box-shadow: 0 5px 18px #284b3509;
}

.progress-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 16px;
}

.progress-heading h2 {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
}

.progress-heading > span {
  color: var(--app-text-secondary);
  font-size: 11px;
}

.directory-row {
  display: flex;
  gap: 10px;
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

.dock-actions {
  display: flex;
  justify-content: space-between;
  align-items: flex-end;
  gap: 20px;
  margin-top: 10px;
}

.dock-status {
  flex: 1;
  min-width: 0;
}

.dock-status p {
  margin: 0;
  color: var(--app-text-secondary);
  font-size: 11px;
  line-height: 1.7;
}

.download-button {
  min-width: 150px;
  height: 44px;
  flex-shrink: 0;
}

@media (max-width: 1000px) {
  .platform-toolbar {
    flex-wrap: wrap;
  }

  .platform-button {
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

  .platform-button {
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

  .dock-actions {
    flex-wrap: wrap;
  }

  .download-button {
    margin-left: auto;
  }
}
</style>
