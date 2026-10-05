<script lang="ts" setup>
import {computed, ref, watch} from "vue";
import {ElButton, ElDrawer, ElIcon, ElScrollbar, ElTooltip} from "element-plus";
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
import {taskIsActive, type DownloadTaskSnapshot} from '../composables/downloadTaskTypes';
import {useThumbnail} from "../composables/useThumbnail";

const props = withDefaults(defineProps<{
  modelValue: boolean;
  record: DownloadRecord | null;
  desktop: boolean;
  busy: boolean;
  downloading: boolean;
  fileRecyclingSupported?: boolean;
  fileDeletionSupported?: boolean;
  error: string | null;
  success: string | null;
  task?: DownloadTaskSnapshot | null
}>(), {fileRecyclingSupported: false, fileDeletionSupported: false, task: null});
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
const active = computed(() => props.task ? taskIsActive(props.task.phase) : ['queued', 'running'].includes(props.record?.status ?? ''));
const systemDisabled = computed(() => !props.desktop || props.busy);
const trashed = computed(() => Boolean(props.record?.deletedAt));
const technical = computed(() => sanitizeHistoryDetail(props.record?.errorDetail));
const hint = computed(() => !props.desktop ? t("history.desktopOnly") : "");
const fileHint = computed(() => !props.desktop ? t("history.desktopOnly") : !props.fileRecyclingSupported ? t("history.fileRecyclingUnavailable") : props.downloading ? t("history.busyHint") : "");
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
        <p class="detail-status">{{ t(`download.platforms.${record.platform}`) }} ·
          {{ t(`history.status.${record.status}`) }}</p>
        <DownloadTaskProgress v-if="task" :task="task"/>
        <div class="detail-actions">
          <ElButton v-if="active&&task" :disabled="systemDisabled||task.phase==='cancelling'" @click="action('cancel')">
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
              :disabled="systemDisabled" @click="action('prepare')">{{ t('history.prepare') }}</ElButton></span>
          </ElTooltip>
        </div>
        <p v-if="!desktop" class="muted">{{ t('history.desktopOnly') }}</p>
        <p v-if="error" class="feedback error" role="alert">{{ error }}</p>
        <p v-if="success" class="feedback" role="status">{{ success }}</p>
        <dl>
          <dt>{{ t('history.quality') }}</dt>
          <dd>{{ record.height ? `${record.height}p` : t('history.unknown') }}</dd>
          <dt>{{ t('history.frameRate') }}</dt>
          <dd>{{ record.fps ? `${record.fps} FPS` : t('history.unknown') }}</dd>
          <dt>{{ t('history.format') }}</dt>
          <dd>{{
              (record.status === 'completed' ? record.outputExtension || record.formatExtension : record.formatExtension)?.toUpperCase() || t('history.unknown')
            }}
          </dd>
          <dt>{{ t('history.duration') }}</dt>
          <dd>{{ historyDuration(record.durationSeconds) || t('history.unknown') }}</dd>
          <dt>{{ t('history.size') }}</dt>
          <dd>{{
              size?.kind === 'actual' ? size.text : size?.kind === 'stream' ? t('history.streamSize', {size: size.text}) : t('history.unknownSize')
            }}
          </dd>
          <dt>{{ t(record.outputPath ? 'history.path' : 'history.directory') }}</dt>
          <dd class="path">{{ record.outputPath || record.downloadDirectory || '—' }}</dd>
          <dt>{{ t('history.source') }}</dt>
          <dd class="path">{{ sanitizeHistoryLink(record.sourceLink) }}</dd>
        </dl>
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
        <section v-if="record.status==='failed'||record.status==='interrupted'" class="failure">
          <h3>{{ t(historyFailureKey(record)) }}</h3>
          <p>{{ t('history.failureStage') }}：{{ t(historyStageKey(record.errorStage)) }}</p>
          <p>{{ t('history.suggestionLabel') }}：{{ t(historySuggestionKey(record)) }}</p>
          <details v-if="record.errorCode||technical">
            <summary>{{ t('history.technical') }}</summary>
            <pre>{{ sanitizeHistoryDetail(record.errorCode) }}<template v-if="technical">{{ '\n' }}{{
                technical
              }}</template></pre>
          </details>
        </section>
        <div v-if="!trashed&&!active" class="detail-actions">
          <ElButton :disabled="systemDisabled" plain type="danger" @click="action('remove')">{{
              t('history.remove')
            }}
          </ElButton>
          <ElTooltip v-if="record.status==='completed'&&record.outputPath" :content="fileHint" :disabled="!fileHint">
            <span><ElButton :disabled="systemDisabled||downloading||!fileRecyclingSupported" plain type="danger"
                            @click="action('removeAndFile')">{{ t('history.removeAndFile') }}</ElButton></span>
          </ElTooltip>
        </div>
        <p v-if="!trashed&&record.status==='completed'&&record.outputPath&&!fileRecyclingSupported" class="muted">
          {{ t('history.fileRecyclingUnavailable') }}</p>
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
  padding: 15px;
  background: var(--app-background);
  border: 1px solid var(--app-border);
  border-radius: 8px;
  margin-bottom: 20px;
}

.failure h3 {
  font-size: 14px;
  margin: 0;
}

.failure p {
  font-size: 13px;
}

.failure summary {
  cursor: pointer;
  font-size: 13px;
}

.failure pre {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  font-size: 12px;
}

.feedback {
  padding: 12px;
  border: 1px solid var(--app-border);
  border-radius: 8px;
  font-size: 13px;
  overflow-wrap: anywhere;
}

.error {
  color: var(--el-color-danger);
}

summary:focus-visible {
  outline: 2px solid var(--app-accent);
}

@media (max-width: 720px) {
  dl {
    grid-template-columns:90px minmax(0, 1fr);
  }
}
</style>
