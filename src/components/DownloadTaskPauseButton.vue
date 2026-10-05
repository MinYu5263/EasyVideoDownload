<script lang="ts" setup>
import {computed, ref} from 'vue';
import {ElButton} from 'element-plus';
import {isTauri} from '@tauri-apps/api/core';
import {useI18n} from 'vue-i18n';
import type {DownloadTaskSnapshot} from '../composables/downloadTaskTypes';
import {useDownloadTasks} from '../composables/useDownloadTasks';

const props = withDefaults(defineProps<{ task: DownloadTaskSnapshot; disabled?: boolean }>(), {disabled: false});
const {t} = useI18n({useScope: 'global'});
const tasks = useDownloadTasks();
const desktop = isTauri();
const controlling = ref(false);
const controllable = computed(() => ['downloading', 'paused'].includes(props.task.phase));

async function togglePause() {
  if (props.disabled || !desktop || controlling.value || !controllable.value) return;
  const paused = props.task.phase === 'paused';
  controlling.value = true;
  try {
    await (paused ? tasks.resume(props.task.record.requestId) : tasks.pause(props.task.record.requestId));
  } finally {
    controlling.value = false;
  }
}
</script>

<template>
  <ElButton v-if="controllable" :aria-pressed="task.phase === 'paused'" :disabled="disabled || controlling || !desktop"
            @click="togglePause">{{ t(task.phase === 'paused' ? 'tasks.resume' : 'tasks.pause') }}
  </ElButton>
</template>
