<script lang="ts" setup>
import {computed, nextTick, onMounted, onUnmounted, ref, watch} from "vue";
import {
  ElButton,
  ElIcon,
  ElInput,
  ElTable,
  ElTableColumn,
  ElScrollbar,
  ElSkeleton,
  ElSkeletonItem,
  ElTooltip,
  type InputInstance,
  type NotificationHandle
} from "element-plus";
import {CircleCheck, Clock, CloseBold, Download, InfoFilled, Loading, RefreshRight} from "@element-plus/icons-vue";
import {invoke, isTauri} from "@tauri-apps/api/core";
import {useI18n} from "vue-i18n";
import PlatformSettings from "./PlatformSettings.vue";
import DownloadProgressControl from "./DownloadProgressControl.vue";
import SegmentedToolbar from "./SegmentedToolbar.vue";
import ContentMotion from "./ContentMotion.vue";
import {createVideoParser, type VideoFormat} from "../composables/useVideoParser";
import {videoCodecLabel, videoQualityLabel} from "../composables/videoFormatTable";
import {formatVideoBitrate, formatVideoSize} from "../composables/videoFormatDisplay";
import {platformIds, type VideoPlatform} from "../composables/videoPlatforms";
import {useDesktopActions} from "../composables/useDesktopActions";
import {useCookieSettings} from "../composables/useCookieSettings";
import {usePlatformSettings} from "../composables/usePlatformSettings";
import {usePlatformDirectories} from "../composables/usePlatformDirectories";
import {videoToolRevision} from "../composables/useRequiredTools";
import {proxySettingsRevision} from "../composables/useProxySettings";
import {useDownloadTasks} from "../composables/useDownloadTasks";
import {createDownloadPageState} from "../composables/useDownloadPageState";
import {useUiPreferences} from "../composables/useUiPreferences";
import {useThumbnail} from "../composables/useThumbnail";
import {taskIsActive} from "../composables/downloadTaskTypes";
import {useFeedback} from "../composables/useFeedback";
import {persistenceError, type PersistenceError} from "../composables/useUiPreferences";

const props = defineProps<{ active: boolean }>();
const emit = defineEmits<{ busyChange: [busy: boolean]; viewHistory: [id: number, trashed?: boolean] }>();
const {t, te} = useI18n({useScope: "global"});
const {notify, notifyError, notifyDownloadFailure, watchError} = useFeedback();
const cookies = useCookieSettings();
const platformSettings = usePlatformSettings();
const directories = usePlatformDirectories();
const desktop = useDesktopActions();
const pagePersistence = createDownloadPageState({desktop: desktop.desktop, invoke});
const uiPreferences = useUiPreferences();
const configurationOpen = ref(false), configurationButton = ref<HTMLButtonElement>();
const contentMotion = ref<InstanceType<typeof ContentMotion>>();
let configurationVersion = 0;
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
    if (initialized.value) void pagePersistence.save(parser.snapshot(selected));
  },
  cacheThumbnail: async (url, platform) => {
    try {
      return await invoke<string>("cache_video_thumbnail", {url, platform});
    } catch (error) {
      notifyError(t("persistence.thumbnailFailed"), {detail: persistenceError(error).detail});
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
  busy: parserBusy,
  pasting
} = parser;
const tasks = useDownloadTasks();
const submitting = ref(false);
const startingFormatId = ref<string | null>(null);
const controllingRequests = ref(new Set<string>());
const recordLoadError = ref<PersistenceError | null>(null);
// Parse and download entry points await proxy saves without locking navigation.
const navigationDisabled = computed(() => !initialized.value || !uiPreferences.ready.value || submitting.value);
const pageBusy = computed(() => parserBusy.value || !initialized.value || !uiPreferences.ready.value);
const busy = computed(() => pageBusy.value || submitting.value);
const coverSource = useThumbnail(displayThumbnailCachePath, computed(() => displayVideo.value?.thumbnail ?? null), error => notifyError(t("persistence.thumbnailReadFailed"), {detail: error.detail}));
const platformOptions = computed(() => platformIds.map(value => ({label: t(`download.platforms.${value}`), value})));
watch(navigationDisabled, value => emit('busyChange', value), {immediate: true});
const downloadUnavailable = computed(() => pageBusy.value || !desktop.desktop || !video.value ||
    !directories.ready[platform.value] || directories.working.value === platform.value || !directories.settings[platform.value].directory.trim());
const downloadDisabled = computed(() => submitting.value || downloadUnavailable.value);

const formatRows = computed(() => displayVideo.value?.formats ?? []);

async function refreshFormatRecords() {
  const selected = platform.value, id = video.value?.id;
  if (!id) return;
  try {
    await tasks.connect();
    await tasks.lookupFormats(selected, id);
    if (platform.value === selected && video.value?.id === id) recordLoadError.value = null;
  } catch (failure) {
    if (platform.value === selected && video.value?.id === id) recordLoadError.value = persistenceError(failure);
  }
}

watch([platform, () => video.value?.id, tasks.recordsRevision], () => {
  recordLoadError.value = null;
  void refreshFormatRecords();
}, {immediate: true});

function formatRecord(row: VideoFormat) {
  return video.value ? tasks.recordForFormat(platform.value, video.value.id, row.formatId) : null;
}

function rowState(row: VideoFormat) {
  if (startingFormatId.value === row.formatId) return 'submitting';
  const record = formatRecord(row);
  if (!record || record.deletedAt) return 'ready';
  const task = tasks.taskFor(record.id);
  if (task && task.record.requestId === record.requestId && taskIsActive(task.phase)) return task.phase;
  if (record.status === 'completed' && (!record.outputPath || record.fileAvailability === 'missing')) return 'ready';
  return record.status === 'running' ? 'downloading' : record.status;
}

function rowIcon(row: VideoFormat) {
  const state = rowState(row);
  return state === 'completed' ? CircleCheck : state === 'queued' ? Clock :
      state === 'submitting' || taskIsActive(state) ? Loading :
          ['failed', 'cancelled', 'interrupted'].includes(state) ? RefreshRight : Download;
}

function rowHint(row: VideoFormat) {
  const state = rowState(row), record = formatRecord(row);
  if (state === 'completed') return t('download.row.completed');
  if (state === 'ready') return t('download.row.start');
  if (state === 'submitting') return t('download.row.submitting');
  const label = t(`tasks.phase.${state}`);
  const task = record ? tasks.taskFor(record.id) : null;
  if (state === 'paused' || state === 'downloading') {
    const progress = task?.percent == null ? label : `${label} · ${Math.min(100, task.percent).toFixed(1)}%`;
    return `${progress} · ${t(state === 'paused' ? 'tasks.resume' : 'tasks.pause')}`;
  }
  if (taskIsActive(state)) return task?.percent == null ? label : `${label} · ${Math.min(100, task.percent).toFixed(1)}%`;
  return `${label} · ${t('download.row.retry')}`;
}

function rowDisabled(row: VideoFormat) {
  const state = rowState(row);
  if (state === 'downloading' || state === 'paused') {
    const record = formatRecord(row);
    return !record || (state === 'downloading' && !tasks.taskFor(record.id)) || controllingRequests.value.has(record.requestId);
  }
  return state === 'submitting' || taskIsActive(state) || (state === 'completed' ? busy.value : downloadDisabled.value);
}

function rowSubmissionLocked(row: VideoFormat) {
  const state = rowState(row);
  return submitting.value && !pageBusy.value && state !== 'submitting' && !taskIsActive(state)
      && (state === 'completed' || !downloadUnavailable.value);
}

function rowPercent(row: VideoFormat) {
  if (['submitting', 'queued', 'preparing'].includes(rowState(row))) return null;
  const record = formatRecord(row);
  return record ? tasks.taskFor(record.id)?.percent ?? null : null;
}

function selectFormatRow(row: VideoFormat) {
  if (busy.value) return;
  const record = formatRecord(row);
  if (record) {
    if (record.deletedAt) emit('viewHistory', record.id, true);
    else emit('viewHistory', record.id);
    return;
  }
  parser.selectFormat(row.formatId);
}

function formatRowClass({row}: { row: VideoFormat }) {
  return row.formatId === displayFormat.value?.formatId ? 'selected-format-row' : '';
}
const coverFailed = ref(false);
const videoLinkInput = ref<InputInstance>();
let parseFailureNotification: NotificationHandle | undefined;
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
  const known = ["invalidLink", "platformMismatch", "desktopOnly", "cookieSaveFailed", "cookieReadFailed", "cookieRequired", "youtubeReloadRequired", "toolMissing", "toolSettingsFailed", "proxySettingsFailed", "spawnFailed", "timeout", "readFailed", "outputTooLarge", "parseFailed", "invalidResult", "unsupportedVideo", "noFormats", "bridgeFailed", "clipboardWriteFailed", "invalidDownloadDirectory", "invalidDownloadOptions", "ffmpegMissing", "denoMissing", "downloadFailed", "downloadResultMissing", "downloadDirectoryFailed", "defaultDirectoryFailed", "downloadTimeout", "downloadBusy", "cancelFailed"];
  if (selected === 'douyin' && failure && !known.includes(failure.code) && te(`download.douyinErrors.${failure.code}`)) return t(`download.douyinErrors.${failure.code}`);
  const code = failure && known.includes(failure.code) ? failure.code : "bridgeFailed";
  return t(`download.errors.${code}`, {platform: t(`download.platforms.${selected}`)});
}

function notifyFailure(failure: {
  code: string;
  detail?: string
}, title: string, selected: VideoPlatform, downloadFailure = false) {
  if (downloadFailure) return notifyDownloadFailure(messageForError(failure, selected), {
    detail: failure.detail,
    key: 'download:operation'
  });
  return notifyError(messageForError(failure, selected), {
    title, detail: failure.detail, key: 'download:operation',
  });
}

async function startDownload(row: VideoFormat) {
  if (rowDisabled(row)) return;
  const record = formatRecord(row);
  const state = rowState(row);
  if (record && (state === 'downloading' || state === 'paused')) {
    controllingRequests.value = new Set([...controllingRequests.value, record.requestId]);
    try {
      await (state === 'paused' ? tasks.resume(record.requestId) : tasks.pause(record.requestId));
    } finally {
      const next = new Set(controllingRequests.value);
      next.delete(record.requestId);
      controllingRequests.value = next;
    }
    return;
  }
  if (!resultLink.value) return;
  if (rowState(row) === 'completed' && record) {
    emit('viewHistory', record.id);
    return;
  }
  if (!video.value?.formats.some(format => format.formatId === row.formatId)) return;
  parser.selectFormat(row.formatId);
  closeParseFailureNotification();
  const selected = platform.value;
  const captured = JSON.parse(JSON.stringify(parser.snapshot(selected)));
  startingFormatId.value = row.formatId;
  submitting.value = true;
  try {
    if (!await platformSettings.whenIdle(selected)) throw {code: 'platformSettingsFailed'};
    if (!await pagePersistence.whenIdle(selected)) throw {code: 'pageSaveFailed'};
    if (!await cookies.whenIdle(selected)) throw {code: 'cookieSaveFailed'};
    if (!await directories.whenIdle(selected)) throw {code: 'invalidDownloadDirectory'};
    const snapshot = directories.applyToSnapshot(captured);
    if (!await pagePersistence.save(snapshot) || !await pagePersistence.whenIdle(selected)) throw {code: 'pageSaveFailed'};
    const request = {snapshot, restoreTrashed: false, redownload: false};
    try {
      await tasks.submit(request);
    } catch (failure) {
      if ((failure as { code: string })?.code !== 'recordTrashed') throw failure;
      await tasks.submit({...request, restoreTrashed: true});
    }
  } catch (failure) {
    parseFailureNotification = notifyFailure(persistenceError(failure), t('download.transfer.failed'), selected, true);
  } finally {
    submitting.value = false;
    startingFormatId.value = null;
  }
}

let restoring = false;

async function restorePageState() {
  if (restoring) return;
  restoring = true;
  try {
    const toolRevision = videoToolRevision.value;
    const proxyRevision = proxySettingsRevision.value;
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
    if (proxyRevision !== proxySettingsRevision.value) parser.resetAll();
    else if (toolRevision !== videoToolRevision.value) parser.invalidateToolFormats();
  } finally {
    restoring = false;
  }
}

onMounted(restorePageState);
onMounted(() => {
  void platformSettings.loadProxy();
  platformIds.forEach(selected => {
    void platformSettings.load(selected);
    void directories.load(selected);
  });
});
watch(uiPreferences.ready, ready => {
  if (ready && !initialized.value) void restorePageState();
});

watchError(pagePersistence.loadError, () => t('persistence.loadFailed'), () => ({key: 'download:load'}));
for (const selected of platformIds) watchError(() => pagePersistence.saveErrors.value[selected], () => t('persistence.saveFailed'), () => ({
  key: `download:save:${selected}`
}));
watchError(recordLoadError, () => t('history.loadFailed'), () => ({key: 'download:records'}));

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
  if (!failure) {
    if (platform.value !== selected) return;
    if (video.value?.cookieFallback) notify(t('download.result.cookieFallback'), 'warning', {key: 'download:cookie-fallback'});
    return;
  }
  if (!parser.isCurrentError(selected, failure)) return;
  parseFailureNotification = notifyFailure(failure,
      `${t(`download.platforms.${selected}`)} · ${t('download.empty.failed')}`, selected);
}

function selectPlatform(value: VideoPlatform) {
  parser.selectPlatform(value);
  if (platform.value === value) void uiPreferences.update({downloadPlatform: value});
}

watch(() => props.active, active => {
  if (!active) {
    parser.cancelPaste();
    configurationOpen.value = false;
  }
});
watch(displayVideo, () => {
  coverFailed.value = false;
});
watch(videoToolRevision, parser.invalidateToolFormats);
watch(proxySettingsRevision, parser.resetAll);
watch(proxySettingsRevision, () => {
  void platformSettings.loadProxy();
});

async function openPlatformSettings() {
  const version = ++configurationVersion;
  configurationOpen.value = !configurationOpen.value;
  parser.cancelPaste();
  await nextTick();
  await contentMotion.value?.whenIdle();
  if (version !== configurationVersion || !props.active) return;
  if (configurationOpen.value) document.getElementById('download-platform-settings')?.focus();
  else configurationButton.value?.focus();
}

async function closePlatformSettings() {
  const version = ++configurationVersion;
  configurationOpen.value = false;
  await nextTick();
  await contentMotion.value?.whenIdle();
  if (version !== configurationVersion || !props.active) return;
  configurationButton.value?.focus();
}

async function pasteLink() {
  try {
    if (await parser.pasteLink(desktop.readClipboard)) {
      await nextTick();
      videoLinkInput.value?.focus();
    }
  } catch (error) {
    notifyError(t("download.link.clipboardReadFailed"), {detail: persistenceError(error).detail});
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
      <SegmentedToolbar :ariaLabel="t('download.choosePlatform')" :disabled="navigationDisabled" :model-value="platform"
                        :options="platformOptions" @update:model-value="selectPlatform">
        <template #actions>
          <button ref="configurationButton" :aria-expanded="configurationOpen"
                  :aria-label="configurationOpen ? t('download.configuration.back') : t('download.configuration.title', {platform: t(`download.platforms.${platform}`)})" :class="{active: configurationOpen}"
                  :disabled="navigationDisabled"
                  aria-controls="download-platform-settings"
                  class="cookie-trigger"
                  type="button"
                  @click="openPlatformSettings">
            {{ t(configurationOpen ? 'download.configuration.back' : 'download.configuration.configure') }}
          </button>
        </template>
      </SegmentedToolbar>
    </div>


    <div class="download-content">
      <ContentMotion ref="contentMotion" :active="props.active"
                     :flip-key="configurationOpen"
                     :position="platformIds.indexOf(platform) * 2 + (configurationOpen ? 1 : 0)" :view-key="`${platform}:${configurationOpen}`">
        <div class="download-panel">
          <ElScrollbar v-if="configurationOpen"
                       :aria-label="t('download.configuration.title', {platform: t(`download.platforms.${platform}`)})"
                       :tabindex="0" class="configuration-scrollbar" height="100%" role="region"
                       view-class="configuration-content">
            <PlatformSettings id="download-platform-settings" :platform="platform" tabindex="-1"
                              @keydown.esc="closePlatformSettings"/>
          </ElScrollbar>
          <div v-else :aria-busy="parserBusy" :class="{'is-parsing': parserBusy}" :inert="parserBusy"
               class="download-card download-workflow">
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
            <div class="result-content">
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
                    <h2 id="parsed-video-title" :title="displayVideo.title">{{ displayVideo.title }}</h2>
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
                <ElTable :data="formatRows" :empty-text="t('download.result.noOptions')" :row-class-name="formatRowClass"
                         class="format-table"
                         height="100%" row-key="formatId"
                         @row-click="selectFormatRow">
                  <ElTableColumn :label="t('download.row.action')" align="center" fixed="left" width="56">
                    <template #default="{row}">
                      <span class="format-download-control">
                          <DownloadProgressControl
                              v-if="taskIsActive(rowState(row as VideoFormat)) || rowState(row as VideoFormat) === 'submitting'"
                              :disabled="rowDisabled(row as VideoFormat)"
                              :label="`${rowHint(row as VideoFormat)} · ${row.formatId}`"
                              :paused="rowState(row as VideoFormat) === 'paused'"
                              :percent="rowPercent(row as VideoFormat)"
                              @click="startDownload(row as VideoFormat)"/>
                          <ElButton v-else :aria-label="`${rowHint(row as VideoFormat)} · ${row.formatId}`"
                                    :class="{'is-submission-locked': rowSubmissionLocked(row as VideoFormat)}" :disabled="rowDisabled(row as VideoFormat)"
                                    :icon="rowIcon(row as VideoFormat)"
                                    :type="rowState(row as VideoFormat) === 'completed' ? 'success' : 'primary'"
                                    circle
                                    @click.stop="startDownload(row as VideoFormat)"/>
                      </span>
                    </template>
                  </ElTableColumn>
                  <ElTableColumn :label="t('download.result.quality')" min-width="90" show-overflow-tooltip>
                    <template #default="{row}">{{
                        videoQualityLabel(row as VideoFormat) ?? t('download.result.unknown')
                      }}<span v-if="row.watermarked === true"
                              class="watermark-label"> · {{ t('download.result.watermarked') }}</span></template>
                  </ElTableColumn>
                  <ElTableColumn :label="t('download.result.resolution')" min-width="104" show-overflow-tooltip>
                    <template #default="{row}">
                      {{ row.width && row.height ? `${row.width} × ${row.height}` : t('download.result.unknown') }}
                    </template>
                  </ElTableColumn>
                  <ElTableColumn :label="t('download.result.frameRate')" min-width="90" show-overflow-tooltip>
                    <template #default="{row}">{{
                        row.fps == null ? t('download.result.unknown') : `${row.fps} fps`
                      }}
                    </template>
                  </ElTableColumn>
                  <ElTableColumn :label="t('download.result.codec')" min-width="90" show-overflow-tooltip>
                    <template #default="{row}">{{
                        videoCodecLabel(row as VideoFormat) ?? t('download.result.unknown')
                      }}
                    </template>
                  </ElTableColumn>
                  <ElTableColumn :label="t('download.result.bitrate')" min-width="90" show-overflow-tooltip>
                    <template #default="{row}">{{
                        formatVideoBitrate(row.bitrate) ?? t('download.result.unknown')
                      }}
                    </template>
                  </ElTableColumn>
                  <ElTableColumn min-width="90" show-overflow-tooltip>
                    <template #header>
                      <span class="size-column-header">{{ t('download.result.fileSize') }}<ElTooltip
                          :content="t('download.result.tableSizeHint')" placement="top"><ElIcon :aria-label="t('download.result.tableSizeHint')"
                                                                                                tabindex="0"><InfoFilled/></ElIcon></ElTooltip></span>
                    </template>
                    <template #default="{row}">{{
                        formatVideoSize(row.sizeBytes) ?? t('download.result.unknown')
                      }}
                    </template>
                  </ElTableColumn>
                </ElTable>
              </div>
            </div>
          </div>
        </section>

          </div>
        </div>
      </ContentMotion>
    </div>
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

.download-content {
  display: flex;
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.download-panel {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  box-sizing: border-box;
  padding: 0 var(--app-page-padding-x) 16px;
}

.configuration-scrollbar {
  width: 100%;
  min-height: 0;
  overflow: hidden;
  border: 1px solid var(--app-border);
  border-radius: var(--app-radius);
  background: var(--app-surface);
}

.configuration-scrollbar :deep(.platform-settings) {
  border: 0;
  border-radius: 0;
}

.configuration-scrollbar :deep(.configuration-content) {
  display: flex;
  flex-direction: column;
  height: 100%;
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
  flex: 1;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
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

.link-card {
  flex-shrink: 0;
}

.result-card {
  display: flex;
  flex: 1;
  min-height: 0;
  flex-direction: column;
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





.result-layout {
  display: flex;
  flex: 1;
  min-height: 0;
}

.result-content {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  gap: 20px;
}

.download-card.is-parsing {
  opacity: .55;
}

.result-media {
  min-height: 88px;
  flex-shrink: 0;
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
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  overflow: hidden;
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
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
  gap: 10px;
}

.format-table {
  width: 100%;
  flex: 1;
  min-height: 0;
}

.format-table :deep(.cell) {
  padding: 0 8px;
}

.format-table :deep(.el-table__row) {
  cursor: pointer;
}

.format-table :deep(.selected-format-row) {
  --el-table-tr-bg-color: var(--app-accent-soft);
}

.format-download-control {
  display: inline-flex;
  vertical-align: middle;
}

.format-download-control .el-button {
  width: 32px;
  height: 32px;
}

.format-download-control .el-button.is-submission-locked:disabled {
  color: var(--el-button-text-color);
  background-color: var(--el-button-bg-color);
  border-color: var(--el-button-border-color);
}

.format-download-control .is-active-task :deep(.el-icon) {
  animation: download-spin 1.2s linear infinite;
}

@keyframes download-spin {
  to {
    transform: rotate(360deg);
  }
}

@media (prefers-reduced-motion: reduce) {
  .format-download-control .is-active-task :deep(.el-icon) {
    animation: none;
  }
}

.size-column-header {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}

.watermark-label {
  color: var(--app-text-secondary);
  font-size: 11px;
}

.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
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
