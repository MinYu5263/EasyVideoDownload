<script setup lang="ts">
import {computed} from "vue";
import {ElButton, ElIcon, ElInput, ElMessage, ElOption, ElSelect} from "element-plus";
import {FolderOpened, TopRight} from "@element-plus/icons-vue";
import {toolWebsites, useDesktopActions} from "../composables/useDesktopActions";
import {useI18n} from "vue-i18n";
import type {RequiredToolId, RequiredToolSource, RequiredToolState} from "../composables/useRequiredTools";

const props = defineProps<{
  toolId: RequiredToolId;
  state: RequiredToolState;
  ready: boolean;
  automaticSupported: boolean;
  requiresRosetta?: boolean
}>();
const emit = defineEmits<{
  check: [];
  choose: [];
  configure: [];
  cancel: [];
  sourceChange: [source: RequiredToolSource]
}>();
const {t} = useI18n({useScope: "global"});
const desktop = useDesktopActions();
const isDirectory = computed(() => props.toolId === "ffmpeg");
const locked = computed(() => !props.ready || props.state.operation !== null);
const automatic = computed(() => props.state.source === "automatic");
// Saved configuration can survive a failed check; show only the current selection.
const currentConfig = computed(() => {
  const {active, source, manualPath, operation, error} = props.state;
  if (!active || operation === "checking" || operation === "configuring") return null;
  if (error && error.code !== "configureCleanupFailed") return null;
  if (active.source !== source || (source === "manual" && active.manualPath !== manualPath.trim())) return null;
  return active;
});
const visibleError = computed(() => automatic.value && props.state.error?.code === "notFound" ? null : props.state.error);
const managedReady = computed(() => currentConfig.value?.source === "automatic");
const needsConfiguration = computed(() => automatic.value && !managedReady.value);
const status = computed(() => {
  if (props.state.operation === "configuring") return "configuring";
  if (props.state.operation === "checking") return "checking";
  if (automatic.value) {
    if (managedReady.value) return "installed";
    if (props.state.error && !["notFound", "configureCancelled"].includes(props.state.error.code)) return "unavailable";
    return "notConfigured";
  }
  return currentConfig.value ? "installed" : props.state.error?.code === "notFound" ? "missing" : visibleError.value ? "unavailable" : "pending";
});
const progressText = computed(() => {
  const progress = props.state.progress;
  if (props.state.cancelling) return t("settings.requiredTools.cancelling");
  if (!progress) return t("settings.requiredTools.configuring");
  if (progress.phase === "downloading" && progress.total && progress.total > 0) {
    return t("settings.requiredTools.downloadProgress", {percent: Math.min(100, Math.floor(progress.downloaded / progress.total * 100))});
  }
  return t(`settings.requiredTools.configurePhases.${progress.phase}`);
});

function sourceChanged(value: unknown) {
  if (value === "path" || value === "manual" || (value === "automatic" && props.automaticSupported)) emit("sourceChange", value);
}

async function openWebsite(event: MouseEvent) {
  if (event.type === "auxclick" && event.button !== 1) return;
  event.preventDefault();
  try {
    await desktop.openToolWebsite(props.toolId);
  } catch {
    ElMessage.error(t("settings.requiredTools.websiteOpenFailed", {url: toolWebsites[props.toolId]}));
  }
}
</script>

<template>
  <section class="required-tool-card" :aria-labelledby="`required-tool-title-${toolId}`"
           :aria-busy="state.operation !== null">
    <header class="required-tool-heading">
      <div class="required-tool-summary">
        <div class="required-tool-title-row">
          <h3 :id="`required-tool-title-${toolId}`">{{ t(`settings.requiredTools.${toolId}.name`) }}</h3>
          <button
              :disabled="!desktop.desktop"
              class="tool-website"
              type="button"
              :title="t('settings.requiredTools.officialWebsite', { program: t(`settings.requiredTools.${toolId}.name`) })"
              :aria-label="t('settings.requiredTools.officialWebsite', { program: t(`settings.requiredTools.${toolId}.name`) })"
              @click="openWebsite"
              @auxclick="openWebsite"
          >
            <ElIcon :size="13" aria-hidden="true">
              <TopRight/>
            </ElIcon>
          </button>
          <span class="required-tool-status" :class="`status-${status}`" role="status">
            <span class="status-dot" aria-hidden="true"></span>
            {{ t(`settings.requiredTools.${status}`) }}
          </span>
        </div>
        <p>{{ t(`settings.requiredTools.${toolId}.description`) }}</p>
      </div>
      <div class="required-tool-controls" role="group"
           :aria-label="t('settings.requiredTools.sourceLabel', { program: t(`settings.requiredTools.${toolId}.name`) })">
        <ElSelect
            :id="`required-tool-source-${toolId}`"
            v-model="state.source"
            :disabled="locked"
            class="source-select"
            :aria-label="t('settings.requiredTools.sourceLabel', { program: t(`settings.requiredTools.${toolId}.name`) })"
            @change="sourceChanged"
        >
          <ElOption value="path" :label="t('settings.requiredTools.systemPath')"/>
          <ElOption v-if="automaticSupported" :label="t('settings.requiredTools.automatic')" value="automatic"/>
          <ElOption value="manual" :label="t('settings.requiredTools.manual')"/>
        </ElSelect>
        <ElButton
            :disabled="locked"
            :aria-label="t(needsConfiguration ? 'settings.requiredTools.configureLabel' : 'settings.requiredTools.checkLabel', { program: t(`settings.requiredTools.${toolId}.name`) })"
            :loading="state.operation === 'checking' || state.operation === 'configuring'"
            @click="needsConfiguration ? emit('configure') : emit('check')"
        >{{
            t(needsConfiguration || state.operation === 'configuring' ? "settings.requiredTools.configure" : "settings.requiredTools.check")
          }}
        </ElButton>
        <ElButton v-if="state.operation === 'configuring'"
                  :disabled="!state.progress || state.progress.phase === 'saving' || state.cancelling"
                  @click="emit('cancel')">
          {{ t("settings.requiredTools.cancel") }}
        </ElButton>
      </div>
    </header>

    <p v-if="automatic && requiresRosetta" class="draft-note">{{ t("settings.requiredTools.ffmpegRosetta") }}</p>
    <p v-if="state.operation === 'configuring'" class="draft-note" role="status">{{ progressText }}</p>

    <div v-if="state.source === 'manual'" class="manual-panel">
      <label class="field-label" :for="`required-tool-manual-path-${toolId}`">
        {{ t(isDirectory ? "settings.requiredTools.directory" : "settings.requiredTools.programPath") }}
      </label>
      <div class="manual-path-row">
        <ElInput
            :id="`required-tool-manual-path-${toolId}`"
            v-model="state.manualPath"
            :disabled="locked"
            :placeholder="t(isDirectory ? 'settings.requiredTools.directoryPlaceholder' : 'settings.requiredTools.pathPlaceholder')"
            :aria-label="t(isDirectory ? 'settings.requiredTools.directoryLabel' : 'settings.requiredTools.pathLabel', { program: t(`settings.requiredTools.${toolId}.name`) })"
            spellcheck="false"
            @keyup.enter="emit('check')"
        />
        <ElButton :icon="FolderOpened" :disabled="locked" :loading="state.operation === 'choosing'"
                  @click="emit('choose')">
          {{ t(isDirectory ? "settings.requiredTools.chooseDirectory" : "settings.requiredTools.chooseProgram") }}
        </ElButton>
      </div>
    </div>

    <p v-if="state.error?.code === 'configureCancelled'" class="draft-note" role="status">
      {{ t("settings.requiredTools.errors.configureCancelled") }}</p>
    <div v-else-if="visibleError" class="required-tool-error" role="alert">
      <p>{{
          t(`settings.requiredTools.errors.${visibleError.code}`, {program: visibleError.program || t(`settings.requiredTools.${toolId}.name`)})
        }}</p>
      <details v-if="visibleError.detail">
        <summary>{{ t("settings.requiredTools.errorDetails") }}</summary>
        <pre>{{ visibleError.detail }}</pre>
      </details>
    </div>

    <dl v-if="currentConfig" class="required-tool-details">
      <template v-for="program in currentConfig.programs" :key="program.name">
        <div>
          <dt>{{ isDirectory ? program.name + ' ' : '' }}{{ t("settings.requiredTools.version") }}</dt>
          <dd>{{ program.version }}</dd>
        </div>
        <div>
          <dt>{{ isDirectory ? program.name + ' ' : '' }}{{ t("settings.requiredTools.programPath") }}</dt>
          <dd>{{ program.path }}</dd>
        </div>
      </template>
    </dl>
  </section>
</template>

<style scoped>
.required-tool-card {
  min-width: 0;
  padding: 20px 22px;
  border: 1px solid var(--app-border);
  border-radius: var(--app-radius);
  background: var(--app-surface);
}

.required-tool-heading {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
}

.required-tool-summary {
  min-width: 0;
}

.required-tool-title-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

.required-tool-title-row h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  line-height: 1.5;
}

.tool-website {
  border: 0;
  padding: 0;
  background: transparent;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  flex-shrink: 0;
  border-radius: 4px;
  color: var(--app-text-muted);
}

.tool-website:hover {
  background: var(--app-accent-soft);
  color: var(--app-accent);
}

.tool-website:focus-visible {
  outline: 2px solid var(--app-accent);
  outline-offset: 2px;
}

.required-tool-heading p {
  margin: 6px 0 0;
  color: var(--app-text-secondary);
  font-size: 12px;
  line-height: 1.7;
}

.required-tool-controls {
  display: flex;
  flex-shrink: 0;
  align-items: center;
  gap: 8px;
}

.source-select {
  width: 150px;
}

.required-tool-status {
  display: inline-flex;
  align-items: center;
  flex-shrink: 0;
  gap: 5px;
  padding: 1px 7px;
  border-radius: 5px;
  background: var(--app-background);
  color: var(--app-text-secondary);
  font-size: 10px;
  line-height: 18px;
  white-space: nowrap;
}

.status-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--app-text-muted);
}

.status-installed {
  background: var(--app-accent-soft);
  color: var(--app-accent);
}

.status-installed .status-dot, .status-checking .status-dot, .status-configuring .status-dot {
  background: var(--app-accent);
}

.status-unavailable, .status-missing {
  background: var(--app-danger-soft);
  color: var(--app-danger);
}

.status-unavailable .status-dot, .status-missing .status-dot {
  background: var(--app-danger);
}

.manual-panel {
  margin-top: 16px;
}

.field-label {
  display: block;
  margin-bottom: 8px;
  color: var(--app-text-secondary);
  font-size: 12px;
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

.required-tool-card :deep(.el-button) {
  flex-shrink: 0;
  font-size: 12px;
}

.required-tool-details {
  display: grid;
  gap: 8px;
  margin: 16px 0 0;
  padding-top: 14px;
  border-top: 1px solid var(--app-border);
  font-size: 11px;
  line-height: 1.7;
}

.required-tool-details > div {
  display: grid;
  grid-template-columns: 104px minmax(0, 1fr);
  gap: 12px;
}

.required-tool-details dt, .required-tool-details dd {
  margin: 0;
  color: var(--app-text-secondary);
}

.required-tool-details dd {
  overflow-wrap: anywhere;
}

.required-tool-error {
  margin-top: 14px;
  padding: 12px;
  border-radius: 8px;
  background: var(--app-danger-soft);
  color: var(--app-danger);
  font-size: 12px;
  line-height: 1.7;
}

.required-tool-error p {
  margin: 0;
}

.required-tool-error details {
  margin-top: 6px;
}

.required-tool-error summary {
  cursor: pointer;
}

.required-tool-error pre {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  font: inherit;
  max-height: 160px;
  overflow: auto;
}

.draft-note {
  margin: 12px 0 0;
  color: var(--app-text-secondary);
  font-size: 11px;
  line-height: 1.7;
}

@media (max-width: 800px) {
  .required-tool-heading {
    flex-wrap: wrap;
    gap: 12px;
  }

  .required-tool-controls {
    width: 100%;
  }

  .source-select {
    flex: 1;
  }

  .manual-path-row {
    flex-wrap: wrap;
    gap: 10px;
  }

  .manual-path-row :deep(.el-input) {
    flex-basis: 100%;
  }
}
</style>
