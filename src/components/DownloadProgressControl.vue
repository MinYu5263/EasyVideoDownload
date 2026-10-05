<script lang="ts" setup>
import {computed} from 'vue';
import {ElIcon, ElLoadingDirective as vLoading, ElProgress} from 'element-plus';
import {VideoPause, VideoPlay} from '@element-plus/icons-vue';

const props = defineProps<{ percent: number | null; paused: boolean; disabled: boolean; label: string }>();
const emit = defineEmits<{ click: [] }>();
const percentage = computed(() => props.percent == null || !Number.isFinite(props.percent) ? 0 : Math.max(0, Math.min(100, props.percent)));
const indeterminate = computed(() => !props.paused && (props.percent === null || !Number.isFinite(props.percent)));
const ringWidth = 28;
const ringStrokeWidth = 3;
// Match ElProgress's SVG geometry so both states share the same visible ring.
const relativeStrokeWidth = (ringStrokeWidth / ringWidth * 100).toFixed(1);
const ringRadius = Math.floor(50 - Number(relativeStrokeWidth) / 2);
const loadingSvg = `<circle class="path" cx="25" cy="25" r="${ringRadius / 2}" fill="none" style="stroke-width: ${Number(relativeStrokeWidth) / 2}" />`;
</script>

<template>
  <button :aria-busy="indeterminate" :aria-label="label" :aria-pressed="paused"
          :class="{paused}"
          :disabled="disabled" :style="{'--download-progress-ring-size': `${ringWidth}px`}" class="download-progress-control" type="button"
          @click.stop="emit('click')">
    <span v-loading="indeterminate" :element-loading-svg="loadingSvg" class="progress-ring"
          element-loading-background="transparent"
          element-loading-svg-view-box="0 0 50 50">
      <ElProgress :aria-valuenow="indeterminate || percent === null ? undefined : percentage" :color="paused ? 'var(--app-text-muted)' : 'var(--app-accent)'" :percentage="percentage" :show-text="false"
                  :stroke-width="ringStrokeWidth"
                  :width="ringWidth"
                  type="circle"/>
    </span>
    <span v-if="!indeterminate || !disabled" class="progress-center">
      <ElIcon v-if="paused" class="resume-icon"><VideoPlay/></ElIcon>
      <template v-else>
        <span v-if="!indeterminate && percent !== null" class="progress-number">{{ Math.floor(percentage) }}</span>
        <ElIcon v-if="!disabled" class="pause-icon"><VideoPause/></ElIcon>
      </template>
    </span>
  </button>
</template>

<style scoped>
.download-progress-control {
  --el-loading-spinner-size: var(--download-progress-ring-size);
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  padding: 0;
  border: 0;
  border-radius: 50%;
  background: transparent;
  color: var(--app-accent);
  cursor: pointer;
}

.download-progress-control:disabled {
  cursor: default;
}

.download-progress-control:focus-visible {
  outline: 2px solid var(--app-accent);
  outline-offset: 3px;
}

.progress-ring {
  display: inline-flex;
  width: var(--download-progress-ring-size);
  height: var(--download-progress-ring-size);
}

.progress-ring :deep(.el-loading-mask) {
  z-index: 1;
  border-radius: 50%;
  pointer-events: none;
}

.progress-ring :deep(.el-loading-spinner) {
  line-height: 0;
}

.progress-center {
  position: absolute;
  inset: 0;
  z-index: 2;
  display: grid;
  place-items: center;
}

.progress-number {
  font-size: 9px;
  font-variant-numeric: tabular-nums;
}

.pause-icon {
  display: none;
}

.download-progress-control:is(:hover, :focus-visible):not(:disabled) .pause-icon {
  display: inline-flex;
}

.download-progress-control:is(:hover, :focus-visible):not(:disabled) .progress-number {
  display: none;
}

.paused {
  color: var(--app-text-secondary);
}

@media (prefers-reduced-motion: reduce) {
  .progress-ring :deep(.el-loading-spinner .circular),
  .progress-ring :deep(.el-loading-spinner .path) {
    animation: none;
  }
}
</style>
