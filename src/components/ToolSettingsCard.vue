<script setup lang="ts">
import { computed, ref } from "vue";
import { ElButton, ElInput, ElOption, ElSelect } from "element-plus";
import { FolderOpened } from "@element-plus/icons-vue";
import { useI18n } from "vue-i18n";

const props = defineProps<{
  toolId: "ytdlp" | "ffmpeg" | "runtime";
}>();

const { t } = useI18n({ useScope: "global" });
const source = ref<"path" | "manual">("path");
const manualPath = ref("");
const isDirectory = computed(() => props.toolId === "ffmpeg");
</script>

<template>
  <section class="tool-card" :aria-labelledby="`tool-title-${toolId}`">
    <header class="tool-heading">
      <div>
        <div class="tool-title-row">
          <h3 :id="`tool-title-${toolId}`">{{ t(`settings.tools.${toolId}.name`) }}</h3>
          <span class="tool-kind">{{ t(`settings.tools.${toolId}.kind`) }}</span>
        </div>
        <p>{{ t(`settings.tools.${toolId}.description`) }}</p>
      </div>
      <span class="tool-status">
        <span class="status-dot" aria-hidden="true"></span>
        {{ t("settings.tools.pending") }}
      </span>
    </header>

    <div
      class="tool-configuration"
      role="group"
      :aria-labelledby="`tool-source-label-${toolId}`"
      :aria-describedby="`tool-source-help-${toolId}`"
    >
      <div class="source-row">
        <label :id="`tool-source-label-${toolId}`" :for="`tool-source-${toolId}`">
          {{ t("settings.tools.source") }}
        </label>
        <ElSelect
          :id="`tool-source-${toolId}`"
          v-model="source"
          class="source-select"
          :aria-label="t('settings.tools.sourceLabel', { tool: t(`settings.tools.${toolId}.name`) })"
        >
          <ElOption value="path" :label="t('settings.tools.systemPath')" />
          <ElOption value="manual" :label="t('settings.tools.manual')" />
        </ElSelect>
      </div>

      <div v-if="source === 'path'" class="path-panel">
        <p :id="`tool-source-help-${toolId}`" class="tool-help">
          {{ t("settings.tools.pathHelp") }}
        </p>
        <ElButton disabled :title="t('settings.tools.previewNotice')">
          {{ t("settings.tools.detectPath") }}
        </ElButton>
      </div>

      <div v-else class="manual-panel">
        <label class="field-label" :for="`tool-manual-path-${toolId}`">
          {{ t(isDirectory ? "settings.tools.directory" : "settings.tools.programPath") }}
        </label>
        <div class="manual-path-row">
          <ElInput
            :id="`tool-manual-path-${toolId}`"
            v-model="manualPath"
            :placeholder="t(isDirectory ? 'settings.tools.directoryPlaceholder' : 'settings.tools.pathPlaceholder')"
            :aria-label="t(isDirectory ? 'settings.tools.directoryLabel' : 'settings.tools.pathLabel', { tool: t(`settings.tools.${toolId}.name`) })"
            :aria-describedby="`tool-source-help-${toolId}`"
            spellcheck="false"
          />
          <ElButton :icon="FolderOpened" disabled :title="t('settings.tools.browseNotice')">
            {{ t(isDirectory ? "settings.tools.chooseDirectory" : "settings.tools.chooseProgram") }}
          </ElButton>
        </div>
        <div class="manual-actions">
          <p :id="`tool-source-help-${toolId}`" class="tool-help">
            {{ t("settings.tools.manualHelp") }}
          </p>
          <ElButton disabled :title="t('settings.tools.previewNotice')">
            {{ t("settings.tools.detectAndUse") }}
          </ElButton>
        </div>
      </div>
    </div>

    <dl class="tool-details">
      <div>
        <dt>{{ t("settings.tools.version") }}</dt>
        <dd>—</dd>
      </div>
      <div>
        <dt>{{ t("settings.tools.resolvedPath") }}</dt>
        <dd>{{ t("settings.tools.notDetected") }}</dd>
      </div>
    </dl>
    <p v-if="toolId === 'runtime'" class="runtime-note">
      {{ t("settings.tools.runtimeNote") }}
    </p>
  </section>
</template>

<style scoped>
.tool-card {
  min-width: 0;
  padding: 22px;
  border: 1px solid var(--app-border);
  border-radius: var(--app-radius);
  background: var(--app-surface);
}

.tool-heading {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  padding-bottom: 16px;
  border-bottom: 1px solid var(--app-border);
}

.tool-title-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

.tool-title-row h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  line-height: 1.5;
}

.tool-kind {
  padding: 0 6px;
  border: 1px solid var(--app-border);
  border-radius: 4px;
  color: var(--app-text-secondary);
  font-size: 10px;
  line-height: 18px;
}

.tool-heading p {
  margin: 6px 0 0;
  color: var(--app-text-secondary);
  font-size: 12px;
  line-height: 1.7;
}

.tool-status {
  display: inline-flex;
  flex-shrink: 0;
  align-items: center;
  gap: 5px;
  padding: 3px 8px;
  border-radius: 20px;
  background: var(--app-background);
  color: var(--app-text-secondary);
  font-size: 11px;
  white-space: nowrap;
}

.status-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--app-text-muted);
}

.tool-configuration {
  padding: 16px 0;
  border-bottom: 1px solid var(--app-border);
}

.source-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
}

.source-row > label,
.field-label {
  color: var(--app-text-secondary);
  font-size: 12px;
}

.source-select {
  width: 180px;
  flex-shrink: 0;
}

.path-panel,
.manual-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
  margin-top: 12px;
}

.tool-help {
  margin: 0;
  color: var(--app-text-secondary);
  font-size: 11px;
  line-height: 1.8;
}

.tool-card :deep(.el-button) {
  flex-shrink: 0;
  font-size: 12px;
}

.manual-panel {
  margin-top: 16px;
}

.field-label {
  display: block;
  margin-bottom: 8px;
}

.manual-path-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.manual-path-row :deep(.el-input) {
  min-width: 0;
}

.manual-path-row :deep(.el-input__inner) {
  font-size: 12px;
}

.tool-details {
  display: grid;
  gap: 8px;
  margin: 16px 0 0;
  font-size: 11px;
  line-height: 1.7;
}

.tool-details > div {
  display: grid;
  grid-template-columns: 100px minmax(0, 1fr);
  gap: 12px;
}

.tool-details dt,
.tool-details dd {
  margin: 0;
  color: var(--app-text-secondary);
}

.tool-details dd {
  overflow-wrap: anywhere;
}

.runtime-note {
  margin: 12px 0 0;
  color: var(--app-text-secondary);
  font-size: 11px;
  line-height: 1.7;
}

@media (max-width: 800px) {
  .tool-heading,
  .path-panel,
  .manual-actions,
  .manual-path-row {
    flex-wrap: wrap;
    gap: 10px;
  }

  .manual-path-row :deep(.el-input) {
    flex-basis: 100%;
  }

  .tool-details > div {
    grid-template-columns: 90px minmax(0, 1fr);
    gap: 8px;
  }
}
</style>
