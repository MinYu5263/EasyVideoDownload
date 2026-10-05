<script lang="ts" setup>
import {computed, ref, watch} from "vue";
import {ElAlert, ElButton, ElDrawer, ElIcon, ElScrollbar, ElTooltip} from "element-plus";
import {Picture} from "@element-plus/icons-vue";
import {useI18n} from "vue-i18n";
import type {DownloadRecord} from "../composables/useDownloadHistory";
import {
  historyDuration,
  historyFailureKey,
  historySize,
  historyStageKey,
  historySuggestionKey,
  sanitizeHistoryDetail,
  sanitizeHistoryLink
} from "../composables/downloadHistoryDisplay";
import DownloadTaskProgress from './DownloadTaskProgress.vue';
import DownloadTaskPauseButton from './DownloadTaskPauseButton.vue';
import {taskIsActive, type DownloadTaskSnapshot} from '../composables/downloadTaskTypes';
import {useThumbnail} from "../composables/useThumbnail";
import {videoCodecLabel, videoQualityLabel} from '../composables/videoFormatTable';
import {formatVideoBitrate} from '../composables/videoFormatDisplay';

const props = withDefaults(defineProps<{
  modelValue: boolean;
  record: DownloadRecord | null;
  desktop: boolean;
  busy: boolean;
  downloading: boolean;
  fileDeletionSupported?: boolean;
  task?: DownloadTaskSnapshot | null
}>(), {fileDeletionSupported: false, task: null});
const emit = defineEmits<{
  "update:modelValue": [value: boolean];
  action: [name: string, record: DownloadRecord];
  closed: []
}>();
const {t} = useI18n({useScope: "global"});
const thumbnail = useThumbnail(computed(() => props.record?.thumbnailCachePath ?? null), computed(() => props.record?.thumbnailUrl ?? null));
const fallback = ref(false), failed = ref(false);
watch(thumbnail, () => {
  fallback.value = false;
  failed.value = false;
});
const image = computed(() => failed.value ? null : fallback.value ? props.record?.thumbnailUrl : thumbnail.value);

function imageError() {
  if (!fallback.value && props.record?.thumbnailUrl && image.value !== props.record.thumbnailUrl) fallback.value = true; else failed.value = true;
}

const size = computed(() => props.record ? historySize(props.record) : null);
const recordedFormat = computed(() => props.record?.formatSnapshot);
const active = computed(() => props.task ? taskIsActive(props.task.phase) : ['queued', 'running'].includes(props.record?.status ?? ''));
const statusLabel = computed(() => props.record ? t(props.task ? `tasks.phase.${props.task.phase}` : `history.status.${props.record.status}`) : '');
const systemDisabled = computed(() => !props.desktop || props.busy);
const trashed = computed(() => Boolean(props.record?.deletedAt));
const failureDetail = computed(() => sanitizeHistoryDetail(props.record?.errorDetail) || sanitizeHistoryDetail(props.record?.errorCode));
const failureDescription = computed(() => {
  const record = props.record;
  if (!record) return "";
  const lines = [
    `${t('history.failureStage')}：${t(historyStageKey(record.errorStage))}`,
    `${t('history.suggestionLabel')}：${t(historySuggestionKey(record))}`,
  ];
  if (failureDetail.value) lines.push(t('history.failureDetails', {detail: failureDetail.value}));
  return lines.join('\n');
});
const hint = computed(() => !props.desktop ? t("history.desktopOnly") : "");
const deletionHint = computed(() => !props.desktop ? t("history.desktopOnly") : !props.fileDeletionSupported ? t("history.fileDeletionUnavailable") : props.downloading ? t("history.busyHint") : "");

function action(name: string) {
  if (props.record) emit("action", name, props.record);
}
</script>
<template>
  <ElDrawer :model-value="modelValue" :title="t('history.details')" body-class="history-details-body"
            size="min(560px, 100vw)" @closed="emit('closed')" @update:model-value="emit('update:modelValue',$event)">
    <ElScrollbar :aria-label="t('history.details')" :tabindex="0" class="details-scrollbar" height="100%"
                 role="region" view-class="history-details-content">
      <div v-if="record" class="history-details">
        <div class="detail-cover"><img v-if="image" :src="image" alt="" @error="imageError"><span v-else><ElIcon
            :size="36"><Picture/></ElIcon>{{ t('history.thumbnail') }}</span></div>
        <h2>{{ record.title || t('history.unknownTitle') }}</h2>
        <p class="detail-status" role="status">{{ t(`download.platforms.${record.platform}`) }} ·
          {{ statusLabel }}</p>
        <DownloadTaskProgress v-if="task" :task="task"/>
        <div class="detail-actions">
          <DownloadTaskPauseButton v-if="active&&task" :disabled="systemDisabled" :task="task"/>
          <ElButton v-if="!trashed&&((active&&task)||['paused','cancelled'].includes(record.status))"
                    :disabled="systemDisabled||task?.phase==='cancelling'" @click="action('cancel')">
            {{ t('download.actions.cancel') }}
          </ElButton>
          <ElButton v-if="trashed" :disabled="systemDisabled||active" type="primary" @click="action('restore')">
            {{ t('history.restore') }}
          </ElButton>
          <ElTooltip v-if="trashed" :content="deletionHint" :disabled="!deletionHint"><span><ElButton :disabled="systemDisabled||downloading||!fileDeletionSupported||active"
                                                                                                      plain
                                                                                                      type="danger"
                                                                                                      @click="action('purge')">{{
              t('history.purge')
            }}</ElButton></span></ElTooltip>
          <ElButton v-if="!trashed&&!active&&record.outputPath" :disabled="systemDisabled||!record.outputPath"
                    type="primary" @click="action('openFile')">{{
              t('history.openFile')
            }}
          </ElButton>
          <ElButton
              v-if="!trashed&&(record.status==='completed'||record.status==='failed'||record.status==='interrupted')"
              :disabled="systemDisabled" @click="action('openFolder')">{{ t('history.openFolder') }}
          </ElButton>
          <ElTooltip v-if="!trashed&&!active" :content="hint" :disabled="!hint"><span><ElButton
              :disabled="systemDisabled" @click="action(record.status==='paused'?'resume':'prepare')">{{
              t(record.status === 'paused' ? 'tasks.resume' : 'history.prepare')
            }}</ElButton></span>
          </ElTooltip>
        </div>
        <p v-if="!desktop" class="muted">{{ t('history.desktopOnly') }}</p>
        <dl>
          <dt>{{ t('history.quality') }}</dt>
          <dd>{{ videoQualityLabel(recordedFormat) ?? '-' }}</dd>
          <dt>{{ t('history.formatId') }}</dt>
          <dd class="path">{{ record.formatSnapshot?.formatId ?? record.formatId }}</dd>
          <dt>{{ t('download.result.resolution') }}</dt>
          <dd>{{
              recordedFormat?.width && recordedFormat.height ? `${recordedFormat.width} × ${recordedFormat.height}` : '-'
            }}
          </dd>
          <dt>{{ t('download.result.codec') }}</dt>
          <dd>{{ videoCodecLabel(record.formatSnapshot) ?? '-' }}</dd>
          <dt>{{ t('download.result.bitrate') }}</dt>
          <dd>{{ formatVideoBitrate(record.formatSnapshot?.bitrate) ?? '-' }}</dd>
          <dt>{{ t('history.frameRate') }}</dt>
          <dd>{{ record.fps ? `${record.fps} fps` : '-' }}</dd>
          <dt>{{ t('history.format') }}</dt>
          <dd>{{
              (record.status === 'completed' ? record.outputExtension || record.formatExtension : record.formatExtension)?.toUpperCase() || t('history.unknown')
            }}
          </dd>
          <dt>{{ t('history.duration') }}</dt>
          <dd>{{ historyDuration(record.durationSeconds) || t('history.unknown') }}</dd>
          <dt>{{ t('history.size') }}</dt>
          <dd>{{
              size && size.kind !== 'unknown' ? size.text : t('history.unknownSize')
            }}
          </dd>
          <dt>{{ t(record.outputPath ? 'history.path' : 'history.directory') }}</dt>
          <dd class="path">{{ record.outputPath || record.downloadDirectory || '—' }}</dd>
          <dt>{{ t('history.source') }}</dt>
          <dd class="path">{{ sanitizeHistoryLink(record.sourceLink) }}</dd>
        </dl>
        <section v-if="record.successfulOutput&&record.status!=='completed'" class="last-output">
          <h3>{{ t('tasks.lastSuccessfulOutput') }}</h3>
          <p>{{ videoQualityLabel(record.successfulOutput.formatSnapshot) ?? t('history.unknown') }} ·
            {{ record.successfulOutput.fps ? `${record.successfulOutput.fps} FPS` : t('history.unknown') }} ·
            {{ record.outputExtension?.toUpperCase() || t('history.unknown') }}</p>
          <p class="path">{{ record.outputPath }}</p>
          <p>{{ record.successfulOutput.finishedAt }}</p>
        </section>
        <div class="detail-actions">
          <ElButton :disabled="systemDisabled" size="small" @click="action('copyLink')">{{
              t('history.copyLink')
            }}
          </ElButton>
          <ElButton :disabled="systemDisabled" size="small" @click="action('openSource')">{{
              t('history.openSource')
            }}
          </ElButton>
        </div>
        <dl>
          <dt>{{ t('history.startedAt') }}</dt>
          <dd>{{ record.startedAt }}</dd>
          <template v-if="record.deletedAt">
            <dt>{{ t('history.removedAt') }}</dt>
            <dd>{{ record.deletedAt }}</dd>
          </template>
        </dl>
        <ElAlert v-if="record.status==='failed'||record.status==='interrupted'" :closable="false"
                 :description="failureDescription" :title="t(historyFailureKey(record))"
                 :type="record.status === 'interrupted' ? 'warning' : 'error'" class="failure"/>
        <div v-if="!trashed&&!active" class="detail-actions">
          <ElButton :disabled="systemDisabled" plain type="danger" @click="action('remove')">{{
              t('history.remove')
            }}
          </ElButton>
          <ElTooltip v-if="record.status==='completed'&&record.outputPath" :content="deletionHint"
                     :disabled="!deletionHint">
            <span><ElButton :disabled="systemDisabled||downloading||!fileDeletionSupported" plain type="danger"
                            @click="action('removeAndFile')">{{ t('history.removeAndFile') }}</ElButton></span>
          </ElTooltip>
        </div>
        <p v-if="!trashed&&record.status==='completed'&&record.outputPath&&!fileDeletionSupported" class="muted">
          {{ t('history.fileDeletionUnavailable') }}</p>
      </div>
    </ElScrollbar>
  </ElDrawer>
</template>
<style scoped>
:global(.el-drawer__body.history-details-body) {
  display: flex;
  flex-direction: column;
  min-height: 0;
  padding: 0;
  overflow: hidden;
}

.details-scrollbar {
  flex: 1;
  min-height: 0;
}

.details-scrollbar :deep(.history-details-content) {
  padding: var(--el-drawer-padding-primary, 20px);
}

.history-details {
  min-width: 0;
}

.detail-cover {
  aspect-ratio: 16/9;
  background: var(--app-accent-soft);
  border-radius: 10px;
  overflow: hidden;
}

.detail-cover img {
  height: 100%;
  width: 100%;
  object-fit: cover;
}

.detail-cover > span {
  height: 100%;
  display: flex;
  flex-direction: column;
  gap: 8px;
  align-items: center;
  justify-content: center;
  color: var(--app-text-muted);
}

h2 {
  font-size: 19px;
  margin: 18px 0 8px;
  overflow-wrap: anywhere;
}

.detail-status, .muted {
  color: var(--app-text-secondary);
  font-size: 13px;
}

.detail-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin: 16px 0;
}

.detail-actions :deep(.el-button+.el-button) {
  margin-left: 0;
}

dl {
  display: grid;
  grid-template-columns:110px minmax(0, 1fr);
  gap: 12px 14px;
  padding: 18px 0;
  border-top: 1px solid var(--app-border);
  margin: 18px 0;
}

dt {
  color: var(--app-text-secondary);
  font-size: 13px;
}

dd {
  margin: 0;
  overflow-wrap: anywhere;
  font-size: 13px;
}

.path {
  white-space: pre-wrap;
  user-select: text;
}

.failure {
  align-items: flex-start;
  margin-bottom: 20px;
}

.failure :deep(.el-alert__content) {
  flex: 1;
  min-width: 0;
}

.failure :deep(.el-alert__title) {
  overflow-wrap: anywhere;
}

.failure :deep(.el-alert__description) {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  user-select: text;
  line-height: 1.6;
}
@media (max-width: 720px) {
  dl {
    grid-template-columns:90px minmax(0, 1fr);
  }
}
</style>
