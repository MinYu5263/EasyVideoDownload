<script lang="ts" setup>
import {computed} from 'vue';
import {ElProgress} from 'element-plus';
import {useI18n} from 'vue-i18n';
import {taskIsActive, type DownloadTaskSnapshot} from '../composables/downloadTaskTypes';
import {formatVideoSize} from '../composables/videoFormatDisplay';

const props = defineProps<{ task: DownloadTaskSnapshot }>();
const {t} = useI18n({useScope: 'global'});
const showPhase = computed(() => taskIsActive(props.task.phase) || props.task.phase !== props.task.record.status);
const percent = computed(() => props.task.phase === 'downloading' && props.task.percent !== null && Number.isFinite(props.task.percent) ? Math.min(100, Math.max(0, props.task.percent)) : null);
const speed = computed(() => props.task.speed !== null && props.task.speed >= 0 && Number.isFinite(props.task.speed) ? `${formatVideoSize(Math.round(props.task.speed)) ?? '0 B'}/s` : '—');
</script>
<template>
  <div v-if="showPhase || task.storageError" class="task-progress">
    <div v-if="showPhase" class="progress-label"><span role="status">{{ t(`tasks.phase.${task.phase}`) }}</span><span
        v-if="task.phase==='downloading'">{{ speed }}<template
        v-if="task.eta!==null"> · {{ t('tasks.eta', {seconds: Math.ceil(task.eta)}) }}</template></span></div>
    <ElProgress v-if="percent!==null" :aria-label="t('download.transfer.stream')" :percentage="Math.round(percent*10)/10"
                :stroke-width="6"/>
    <p v-if="task.storageError" class="storage-error" role="alert">{{ t('history.saveFailed') }}</p>
  </div>
</template>
<style scoped>
.task-progress {
  min-width: 0;
}

.progress-label {
  display: flex;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 8px;
  font-size: 12px;
  color: var(--app-text-secondary);
  margin-bottom: 6px;
}

.storage-error {
  color: var(--el-color-danger);
  font-size: 12px;
}
</style>
