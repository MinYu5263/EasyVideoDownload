<script lang="ts" setup>
import {computed} from 'vue';
import {ElProgress} from 'element-plus';
import {useI18n} from 'vue-i18n';
import type {DownloadTaskSnapshot} from '../composables/downloadTaskTypes';

const props = defineProps<{ task: DownloadTaskSnapshot }>();
const {t} = useI18n({useScope: 'global'});
const controllable = computed(() => ['downloading', 'paused'].includes(props.task.phase));
const percent = computed(() => controllable.value && props.task.percent !== null && Number.isFinite(props.task.percent) ? Math.min(100, Math.max(0, props.task.percent)) : null);
const indeterminate = computed(() => ['queued', 'preparing'].includes(props.task.phase) || (props.task.phase === 'downloading' && percent.value === null));
</script>
<template>
  <div v-if="indeterminate || percent !== null" class="task-progress">
    <ElProgress :aria-label="indeterminate ? t(`tasks.phase.${task.phase}`) : t('download.transfer.stream')"
                :aria-valuenow="indeterminate ? undefined : Math.round((percent ?? 0) * 10) / 10"
                :duration="1"
                :indeterminate="indeterminate" :percentage="indeterminate ? 50 : Math.round((percent ?? 0) * 10) / 10" :show-text="!indeterminate" :stroke-width="6"/>
  </div>
</template>
<style scoped>
.task-progress {
  min-width: 0;
}

@media (prefers-reduced-motion: reduce) {
  .task-progress :deep(.el-progress-bar__inner--indeterminate) {
    animation: none;
  }
}
</style>
