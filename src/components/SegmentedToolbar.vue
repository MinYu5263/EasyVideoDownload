<script generic="T extends string | number | boolean" lang="ts" setup>
import {ElSegmented} from "element-plus";
import "element-plus/es/components/segmented/style/css";

withDefaults(defineProps<{
  options: { label: string; value: T; disabled?: boolean }[];
  ariaLabel: string;
  disabled?: boolean;
}>(), {disabled: undefined});
const value = defineModel<T>({required: true});
const emit = defineEmits<{ change: [value: T] }>();
</script>

<template>
  <div class="segmented-toolbar">
    <ElSegmented v-model="value" :aria-label="ariaLabel" :disabled="disabled" :options="options"
                 class="segmented-control" @change="emit('change', $event)">
      <template v-if="$slots.default" #default="{item}">
        <slot :item="item"/>
      </template>
    </ElSegmented>
    <div v-if="$slots.actions" class="segmented-toolbar-actions">
      <slot name="actions"/>
    </div>
  </div>
</template>

<style scoped>
.segmented-toolbar {
  display: flex;
  flex-shrink: 0;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  box-sizing: border-box;
  width: fit-content;
  max-width: 100%;
  padding: 8px;
  border: 1px solid var(--app-border);
  border-radius: var(--app-radius);
  background: var(--app-surface);
}

.segmented-toolbar:has(> .segmented-toolbar-actions:not(:empty)) {
  width: 100%;
}

.segmented-control {
  --el-segmented-color: var(--app-text-secondary);
  --el-segmented-bg-color: transparent;
  --el-segmented-padding: 0;
  --el-segmented-item-selected-color: var(--app-accent);
  --el-segmented-item-selected-bg-color: var(--app-accent-soft);
  --el-segmented-item-selected-disabled-bg-color: var(--app-accent-soft);
  --el-segmented-item-hover-color: var(--app-accent);
  --el-segmented-item-hover-bg-color: var(--app-hover);
  --el-segmented-item-active-bg-color: var(--app-accent-soft);
  --el-border-radius-base: 11px;

  min-width: 0;
  max-width: 100%;
  min-height: 44px;
  overflow-x: auto;
  font-size: 13px;
}

.segmented-control :deep(.el-segmented__group) {
  gap: 4px;
  /* Keep labels at their natural width and clip the rounded selection background. */
  min-width: max-content;
  overflow: hidden;
}

.segmented-control :deep(.el-segmented__item) {
  flex: 0 0 auto;
  min-height: 44px;
  padding: 10px 18px;
}

.segmented-control :deep(.el-segmented__item.is-selected) {
  font-weight: 600;
}

.segmented-control :deep(.el-segmented__item.is-disabled),
.segmented-control :deep(.el-segmented__item-selected.is-disabled) {
  opacity: .55;
}

.segmented-control :deep(.el-segmented__item-selected.is-focus-visible::before) {
  outline-color: var(--app-accent);
  outline-offset: -2px;
}

.segmented-toolbar-actions {
  display: flex;
  align-items: center;
  margin-left: auto;
  padding-left: 12px;
  border-left: 1px solid var(--app-border);
}

.segmented-toolbar-actions:empty {
  display: none;
}

@media (max-width: 1000px) {
  .segmented-control :deep(.el-segmented__item) {
    padding: 10px 14px;
  }
}

@media (max-width: 800px) {
  .segmented-control :deep(.el-segmented__item) {
    padding: 10px 12px;
  }
}

@media (prefers-reduced-motion: reduce) {
  .segmented-control :deep(.el-segmented__item-selected),
  .segmented-control :deep(.el-segmented__item-label) {
    transition: none;
  }
}
</style>
