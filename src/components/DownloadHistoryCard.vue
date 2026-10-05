<script lang="ts" setup>
import {computed, ref, watch} from "vue";
import {ElButton, ElDropdown, ElDropdownItem, ElDropdownMenu, ElIcon, ElTooltip} from "element-plus";
import {
  CircleCheck,
  CircleClose,
  Clock,
  Delete,
  FolderOpened,
  MoreFilled,
  Picture,
  RefreshRight,
  Remove,
  VideoPause,
  Warning
} from "@element-plus/icons-vue";
import {useI18n} from "vue-i18n";
import type {DownloadRecord} from "../composables/useDownloadHistory";
import {historyDuration, historySize} from "../composables/downloadHistoryDisplay";
import DownloadTaskProgress from './DownloadTaskProgress.vue';
import DownloadTaskPauseButton from './DownloadTaskPauseButton.vue';
import {taskIsActive, type DownloadTaskSnapshot} from '../composables/downloadTaskTypes';
import {useThumbnail} from "../composables/useThumbnail";
import {videoQualityLabel} from '../composables/videoFormatTable';
import {formatVideoSize} from '../composables/videoFormatDisplay';

const props = withDefaults(defineProps<{
  record: DownloadRecord;
  desktop: boolean;
  downloading: boolean;
  busy: boolean;
  fileDeletionSupported?: boolean;
  task?: DownloadTaskSnapshot | null;
  mode?: 'history' | 'current'
}>(), {fileDeletionSupported: false, task: null, mode: 'history'});
const emit = defineEmits<{ action: [name: string, record: DownloadRecord] }>();
const {t} = useI18n({useScope: "global"});
const recordedFormat = computed(() => props.record.formatSnapshot);
const cache = computed(() => props.record.thumbnailCachePath), remote = computed(() => props.record.thumbnailUrl);
const thumbnail = useThumbnail(cache, remote), fallback = ref(false), failed = ref(false);
watch(thumbnail, () => {
  fallback.value = false;
  failed.value = false;
});
const image = computed(() => failed.value ? null : fallback.value ? remote.value : thumbnail.value);

function imageError() {
  if (!fallback.value && remote.value && image.value !== remote.value) fallback.value = true; else failed.value = true;
}

const size = computed(() => historySize(props.record));
const extension = computed(() => props.record.status === 'completed' ? props.record.outputExtension ?? props.record.formatExtension : props.record.formatExtension);
const duration = computed(() => historyDuration(props.record.durationSeconds));
const status = computed(() => props.task?.phase ?? props.record.status);
const statusLabel = computed(() => t(props.task ? `tasks.phase.${props.task.phase}` : `history.status.${props.record.status}`));
const speed = computed(() => {
  const value = props.task?.speed;
  return value != null && Number.isFinite(value) && value >= 0 ? `${formatVideoSize(Math.round(value)) ?? '0 B'}/s` : '—';
});
const eta = computed(() => {
  const value = props.task?.eta;
  return value != null && Number.isFinite(value) && value >= 0 ? Math.ceil(value) : null;
});
const icon = computed(() => ({
  queued: Clock,
  running: Clock,
  preparing: Clock,
  downloading: Clock,
  paused: VideoPause,
  processing: Clock,
  cancelling: Clock,
  completed: CircleCheck,
  failed: CircleClose,
  cancelled: Remove,
  interrupted: Warning
})[status.value]);
const active = computed(() => props.task ? taskIsActive(props.task.phase) : ['queued', 'running'].includes(props.record.status));
const systemDisabled = computed(() => !props.desktop || props.busy);
const trashed = computed(() => Boolean(props.record.deletedAt));
const prepareDisabled = computed(() => systemDisabled.value);
const prepareHint = computed(() => !props.desktop ? t("history.desktopOnly") : "");
const deletionHint = computed(() => !props.desktop ? t("history.desktopOnly") : !props.fileDeletionSupported ? t("history.fileDeletionUnavailable") : props.downloading ? t("history.busyHint") : "");

function action(name: string) {
  emit("action", name, props.record);
}
</script>
<template>
  <article :aria-label="`${record.title || t('history.unknownTitle')} · ${statusLabel}`"
           class="history-card">
    <button :aria-label="`${t('history.details')} · ${record.title || t('history.unknownTitle')}`" class="card-main"
            type="button"
            @click="action('details')">
      <span class="cover">
        <img v-if="image" :src="image" alt="" loading="lazy" @error="imageError">
        <span v-else class="cover-placeholder"><ElIcon :size="26"><Picture/></ElIcon><span>{{
            t('history.thumbnail')
          }}</span></span>
        <span v-if="duration" class="duration">{{ duration }}</span>
      </span>
      <span class="record-info">
        <span :title="record.title || t('history.unknownTitle')"
              class="title">{{ record.title || t('history.unknownTitle') }}</span>
        <span class="specs">
          <span v-if="videoQualityLabel(recordedFormat)">{{ videoQualityLabel(recordedFormat) }}</span><span
            v-if="record.fps">{{ record.fps }} fps</span>
          <span v-if="extension">{{ extension?.toUpperCase() }}</span>
          <span>{{
              size.kind === 'unknown' ? t('history.unknownSize') : size.text
            }}</span>
        </span>
        <span class="source">{{ t(`download.platforms.${record.platform}`) }} · {{
            record.startedAt.slice(11, 16) || '—'
          }}</span>
      </span>
    </button>
    <DownloadTaskProgress v-if="task" :task="task"/>
    <div class="record-controls">
      <div class="record-state">
        <span :class="status" class="status" role="status"><ElIcon><component
            :is="icon"/></ElIcon>{{ statusLabel }}</span>
        <span v-if="task?.phase === 'downloading'" class="transfer-stats">{{ speed }}<template
            v-if="eta !== null"> · {{ t('tasks.eta', {seconds: eta}) }}</template></span>
      </div>
      <div class="actions">
        <ElButton v-if="!trashed&&status==='cancelled'" :disabled="systemDisabled" size="small"
                  @click="action('cancel')">{{ t('download.actions.cancel') }}
        </ElButton>
        <ElButton v-if="trashed" :disabled="systemDisabled||active" size="small" type="primary"
                  @click="action('restore')">{{ t('history.restore') }}
        </ElButton>
        <ElTooltip v-if="trashed" :content="deletionHint" :disabled="!deletionHint"><span><ElButton :disabled="systemDisabled||downloading||!fileDeletionSupported||active" plain
                                                                                                    size="small"
                                                                                                    type="danger"
                                                                                                    @click="action('purge')">{{
            t('history.purge')
          }}</ElButton></span></ElTooltip>
        <template v-if="!trashed&&!active&&record.status==='paused'">
          <ElButton :disabled="systemDisabled" size="small" @click="action('resume')">{{ t('tasks.resume') }}</ElButton>
          <ElButton :disabled="systemDisabled" size="small" @click="action('cancel')">{{
              t('download.actions.cancel')
            }}
          </ElButton>
        </template>
        <ElTooltip v-else-if="!trashed&&!active&&Boolean(record.outputPath)" :content="t('history.desktopOnly')"
                   :disabled="desktop">
          <span><ElButton :disabled="systemDisabled || !record.outputPath" size="small" type="primary"
                          @click="action('openFile')">{{ t('history.openFile') }}</ElButton></span>
        </ElTooltip>
        <template v-else-if="!trashed&&active&&task">
          <DownloadTaskPauseButton :disabled="systemDisabled" :task="task" size="small"/>
          <ElButton :disabled="systemDisabled||task.phase==='cancelling'" size="small"
                  @click="action('cancel')">{{ t('download.actions.cancel') }}
          </ElButton>
        </template>
        <ElButton v-else-if="!trashed&&active" size="small" @click="action('details')">{{
            t('history.details')
          }}
        </ElButton>
        <ElTooltip v-else-if="!trashed" :content="prepareHint" :disabled="!prepareHint">
          <span><ElButton :disabled="prepareDisabled" :icon="RefreshRight" size="small"
                          @click="action('prepare')">{{ t('history.prepare') }}</ElButton></span>
        </ElTooltip>
        <ElButton
            v-if="!trashed&&!active&&record.outputPath&&['failed','cancelled','interrupted'].includes(record.status)"
            :disabled="prepareDisabled" size="small" @click="action('prepare')">{{ t('history.prepare') }}
        </ElButton>
        <ElTooltip v-if="!trashed&&!active&&Boolean(record.outputPath)"
                   :content="desktop?t('history.openFolder'):t('history.desktopOnly')">
          <span><ElButton :aria-label="t('history.openFolder')" :disabled="systemDisabled" :icon="FolderOpened"
                          size="small" @click="action('openFolder')"/></span>
        </ElTooltip>
        <ElTooltip v-if="mode==='history'&&!trashed&&!active"
                   :content="desktop?t('history.removeHint'):t('history.desktopOnly')">
          <span><ElButton :aria-label="t('history.remove')" :disabled="systemDisabled" :icon="Delete" size="small"
                          @click="action('remove')"/></span>
        </ElTooltip>
        <ElDropdown trigger="click" @command="action">
          <ElButton :aria-label="t('history.more')" :icon="MoreFilled" size="small"/>
          <template #dropdown>
            <ElDropdownMenu>
              <ElDropdownItem v-if="trashed||!active" command="details">{{ t('history.details') }}</ElDropdownItem>
              <ElDropdownItem v-if="!trashed&&!active&&Boolean(record.outputPath)" :disabled="prepareDisabled"
                              command="prepare">{{ t('history.prepare') }}
              </ElDropdownItem>
              <ElDropdownItem :disabled="systemDisabled" command="copyLink">{{ t('history.copyLink') }}</ElDropdownItem>
              <ElDropdownItem :disabled="systemDisabled" command="openSource">{{
                  t('history.openSource')
                }}
              </ElDropdownItem>
            </ElDropdownMenu>
          </template>
        </ElDropdown>
        <ElButton v-if="mode==='current'&&!active" size="small" @click="action('dismiss')">{{
            t('tasks.dismiss')
          }}
        </ElButton>
      </div>
    </div>
  </article>
</template>
<style scoped>
.history-card {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 15px;
  border: 1px solid var(--app-border);
  border-radius: 12px;
  background: var(--app-surface);
  transition: border-color .15s;
  min-width: 0;
}

.history-card:hover {
  border-color: var(--el-color-primary-light-5);
}

.card-main {
  display: flex;
  gap: 16px;
  align-items: center;
  width: 100%;
  min-width: 0;
  border: 0;
  padding: 0;
  border-radius: 8px;
  background: none;
  color: var(--app-text);
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.cover {
  display: block;
  width: 144px;
  aspect-ratio: 16/9;
  flex-shrink: 0;
  position: relative;
  overflow: hidden;
  border-radius: 8px;
  background: var(--app-accent-soft);
  color: var(--app-text-muted);
}

.cover img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.cover-placeholder {
  display: flex;
  height: 100%;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  font-size: 11px;
}

.duration {
  position: absolute;
  bottom: 5px;
  right: 5px;
  padding: 1px 5px;
  background: #000a;
  color: white;
  font-size: 11px;
  border-radius: 4px;
}

.record-info {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 7px;
  min-width: 0;
}

.title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 15px;
  font-weight: 600;
  line-height: 1.5;
}

.specs {
  display: flex;
  gap: 10px;
  min-width: 0;
  font-size: 12px;
  color: var(--app-text-secondary);
}

.specs > span {
  flex-shrink: 0;
  white-space: nowrap;
}

.specs > span:last-child {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.source {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: var(--app-text-muted);
}

.record-controls {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 12px;
  min-width: 0;
  padding-top: 12px;
  border-top: 1px solid var(--app-border);
}

.record-state {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px 12px;
  min-width: 0;
}

.transfer-stats {
  color: var(--app-text-secondary);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.status {
  display: flex;
  gap: 5px;
  align-items: center;
  font-size: 12px;
  font-weight: 600;
  white-space: nowrap;
}

.status.queued, .status.running, .status.preparing, .status.downloading, .status.processing, .status.cancelling {
  color: #268b7b;
}

.status.completed {
  color: var(--app-accent);
}

.status.failed {
  color: var(--el-color-danger);
}

.status.cancelled, .status.paused {
  color: var(--app-text-muted);
}

.status.interrupted {
  color: var(--el-color-warning);
}

.actions {
  display: flex;
  gap: 7px;
  align-items: center;
  margin-left: auto;
}

.actions :deep(.el-button + .el-button) {
  margin-left: 0;
}

.busy-hint {
  flex-basis: 100%;
  text-align: right;
  color: var(--app-text-secondary);
  font-size: 11px;
}


button:focus-visible {
  outline: 2px solid var(--app-accent);
  outline-offset: 3px;
}

@media (max-width: 720px) {
  .cover {
    width: 110px;
  }

  .card-main {
    gap: 12px;
  }

  .history-card {
    gap: 12px;
    padding: 12px;
  }

  .actions {
    flex-wrap: wrap;
  }
}
</style>
