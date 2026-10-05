<script lang="ts" setup>
import {computed, onMounted, onUnmounted, watch} from 'vue';
import {useI18n} from 'vue-i18n';
import {useDownloadTasks} from '../composables/useDownloadTasks';
import {createTaskCardVisibility} from '../composables/useTaskCardVisibility';
import type {DownloadRecord} from '../composables/useDownloadHistory';
import DownloadHistoryCard from './DownloadHistoryCard.vue';

const props = defineProps<{ active: boolean; desktop: boolean; busy: boolean }>();
const emit = defineEmits<{ action: [name: string, record: DownloadRecord] }>();
const {t} = useI18n({useScope: 'global'}), tasks = useDownloadTasks();
const visibility = createTaskCardVisibility({
  clock: {
    setTimeout: (fn, ms) => window.setTimeout(fn, ms),
    clearTimeout: id => window.clearTimeout(id)
  }
});

function syncVisibility() {
  visibility.setVisible(props.active && (typeof document === 'undefined' || !document.hidden));
}

watch(() => props.active, syncVisibility, {immediate: true});
onMounted(() => document.addEventListener('visibilitychange', syncVisibility));
watch(() => [tasks.tasks.value, tasks.redownloadErrors.value, Object.keys(tasks.tasks.value).map(id => tasks.isSubmitting(Number(id)))], () => {
  for (const task of Object.values(tasks.tasks.value)) {
    visibility.setRetryHeld(task.record.id, tasks.isSubmitting(task.record.id) || Boolean(tasks.redownloadErrors.value[task.record.id]));
    visibility.update(task);
  }
}, {immediate: true});
const cards = computed(() => Object.values(tasks.tasks.value).filter(task => !visibility.isHidden(task.record.id)).sort((a, b) => b.submissionOrder - a.submissionOrder));
const hovered = new Set<number>(), focused = new Set<number>();

function interact(id: number, kind: 'hover' | 'focus', value: boolean) {
  const set = kind === 'hover' ? hovered : focused;
  if (value) set.add(id); else set.delete(id);
  visibility.setInteracting(id, hovered.has(id) || focused.has(id));
}

function focusOut(id: number, event: FocusEvent) {
  if (event.currentTarget instanceof HTMLElement && event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget)) return;
  interact(id, 'focus', false);
}

function action(name: string, record: DownloadRecord) {
  if (name === 'dismiss') visibility.dismiss(record.id); else emit('action', name, record);
}

onUnmounted(() => {
  document.removeEventListener('visibilitychange', syncVisibility);
  visibility.dispose();
});
</script>
<template>
  <section v-if="cards.length" :aria-label="t('tasks.title')" class="current-tasks">
    <h2>{{ t('tasks.title') }} <span>{{ cards.length }}</span></h2>
    <TransitionGroup class="task-cards" name="task-card" tag="div">
      <div v-for="task in cards" :key="task.record.requestId" @focusin="interact(task.record.id,'focus',true)"
           @focusout="focusOut(task.record.id,$event)" @mouseenter="interact(task.record.id,'hover',true)"
           @mouseleave="interact(task.record.id,'hover',false)">
        <DownloadHistoryCard :busy="busy||tasks.isSubmitting(task.record.id)" :desktop="desktop" :downloading="false" :record="task.record"
                             :task="task" mode="current"
                             @action="action"/>
      </div>
    </TransitionGroup>
  </section>
</template>
<style scoped>
.current-tasks {
  min-width: 0;
  margin-top: 22px;
}

.current-tasks h2 {
  font-size: 14px;
  margin: 0 0 12px;
}

.current-tasks h2 span {
  font-weight: 400;
  color: var(--app-text-secondary);
  margin-left: 6px;
}

.task-cards {
  display: flex;
  flex-direction: column;
  gap: 12px;
  position: relative;
}

.task-card-enter-active, .task-card-leave-active, .task-card-move {
  transition: opacity .25s ease, transform .25s ease;
}

.task-card-enter-from {
  opacity: 0;
  transform: translateY(-8px);
}

.task-card-leave-to {
  opacity: 0;
  transform: translateX(100%);
}

.task-card-leave-active {
  position: absolute;
  width: 100%;
}

@media (prefers-reduced-motion: reduce) {
  .task-card-enter-active, .task-card-leave-active, .task-card-move {
    transition: none;
  }

  .task-card-enter-from, .task-card-leave-to {
    transform: none;
  }
}
</style>
