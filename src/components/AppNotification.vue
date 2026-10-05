<script lang="ts" setup>
import {onBeforeUnmount, onMounted, ref} from "vue";
import {ElAlert, ElNotification, ElText} from "element-plus";

withDefaults(defineProps<{
  title: string;
  description: string;
  type?: "primary" | "success" | "info" | "warning" | "error";
}>(), {type: "info"});

const content = ref<HTMLDivElement | null>(null);
let resizeObserver: ResizeObserver | undefined;
onMounted(() => {
  if (!content.value || typeof ResizeObserver === "undefined") return;
  // Keep stacked notices aligned when their content or available width changes.
  resizeObserver = new ResizeObserver(() => ElNotification.updateOffsets("top-right"));
  resizeObserver.observe(content.value);
});
onBeforeUnmount(() => resizeObserver?.disconnect());
</script>

<template>
  <div ref="content" class="notification-content">
    <ElAlert :closable="false"
             :description="description" :title="title" :type="type" class="notification-alert" role="presentation"
             show-icon>
      <template #title>
        <ElText :title="title" class="notification-title" tag="span" truncated>{{ title }}</ElText>
      </template>
      <template #default>
        <ElText class="notification-description" truncated>{{ description }}</ElText>
      </template>
    </ElAlert>
  </div>
</template>

<style scoped>
.notification-alert {
  --el-alert-padding: 14px 36px 14px 16px;
  height: 76px;
  align-items: flex-start;
  border-radius: 0;
}

.notification-alert :deep(.el-alert__content) {
  flex: 1;
  min-width: 0;
}

.notification-alert :deep(.el-alert__icon) {
  flex-shrink: 0;
}

.notification-title.el-text {
  display: block;
  width: 100%;
  margin: 0;
  color: inherit;
  font-size: inherit;
  font-weight: 700;
  line-height: 24px;
}

.notification-alert :deep(.el-alert__description) {
  line-height: 20px;
  user-select: text;
}

.notification-description.el-text {
  display: block;
  width: 100%;
  height: 20px;
  color: inherit;
  line-height: 20px;
  user-select: text;
}

</style>
