<script lang="ts" setup>
import {computed, inject, nextTick, onUnmounted, provide, ref, watch} from 'vue';
import {createContentMotion, type MotionDirection} from '../composables/contentMotion';
import {contentMotionAllowed} from '../composables/motionContext';

const props = withDefaults(defineProps<{
  viewKey: string | number | boolean;
  position?: number;
  axis?: 'horizontal' | 'vertical';
  flipKey?: boolean;
  active?: boolean;
}>(), {position: 0, axis: 'horizontal', active: true});
const content = ref<HTMLElement>(), moving = ref(false);
const parentAllowed = inject(contentMotionAllowed, computed(() => true));
const allowed = computed(() => parentAllowed.value && props.active);
provide(contentMotionAllowed, computed(() => allowed.value && !moving.value));
const motion = createContentMotion(() => content.value,
    () => typeof window === 'undefined' || window.matchMedia('(prefers-reduced-motion: reduce)').matches,
    value => {
      moving.value = value;
    });
let version = 0;
watch(() => props.viewKey, async () => {
  const current = ++version;
  if (!allowed.value) {
    motion.cancel();
    return;
  }
  const direction: MotionDirection = props.axis === 'vertical'
      ? props.position >= previousPosition ? 'up' : 'down'
      : props.position >= previousPosition ? 'left' : 'right';
  const flip = props.flipKey !== previousFlip;
  motion.capture(direction, flip ? 'flip' : 'slide');
  previousPosition = props.position;
  previousFlip = props.flipKey;
  await nextTick();
  if (current === version && allowed.value) motion.play();
});
let previousPosition = props.position, previousFlip = props.flipKey;
watch(allowed, value => {
  if (!value) {
    version++;
    motion.cancel();
  }
  previousPosition = props.position;
  previousFlip = props.flipKey;
}, {flush: 'sync'});
onUnmounted(motion.cancel);
defineExpose({whenIdle: motion.whenIdle, moving});
</script>

<template>
  <div class="content-motion">
    <div ref="content" class="content-motion-body">
      <slot/>
    </div>
  </div>
</template>

<style scoped>
.content-motion {
  position: relative;
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}

.content-motion-body {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
}
</style>
