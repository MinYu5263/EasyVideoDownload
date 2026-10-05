<script lang="ts" setup>
import {onMounted, onUnmounted} from "vue";
import {ElButton, ElCheckbox, ElDialog} from "element-plus";
import {invoke} from "@tauri-apps/api/core";
import {listen} from "@tauri-apps/api/event";
import {useI18n} from "vue-i18n";
import {useAppSettings} from "../composables/useAppSettings";
import {createClosePrompt, type ClosePromptState} from "../composables/useClosePrompt";
import {useFeedback} from "../composables/useFeedback";

const {t} = useI18n({useScope: "global"});
const settings = useAppSettings();
const prompt = createClosePrompt({
  desktop: settings.desktop, invoke,
  listen: handler => listen<ClosePromptState>("app-close-requested", event => handler(event.payload))
}, settings);
const {open, remember, allowBackground, hasActiveTasks, busy, error} = prompt;
const {closeBackgroundMode} = settings;
const {watchError} = useFeedback();
watchError(error, failure => t(`settings.closePrompt.${failure.code}`), failure => ({key: failure.code === 'saveFailed' ? 'app-settings:save' : 'close-window'}));
onMounted(prompt.start);
onUnmounted(prompt.dispose);

function cancel() {
  void prompt.choose("cancel");
}
</script>

<template>
  <ElDialog :before-close="cancel" :close-on-click-modal="false" :close-on-press-escape="!busy" :model-value="open"
            :show-close="!busy" :title="t('settings.closePrompt.title')"
            append-to-body class="close-window-dialog" width="min(500px, calc(100vw - 40px))">
    <p v-if="hasActiveTasks" class="close-description">{{ t('settings.closePrompt.tasksRunning') }}</p>
    <template #footer>
      <div class="close-footer">
        <ElCheckbox v-if="allowBackground" v-model="remember" :disabled="Boolean(busy) || !settings.ready.value">
          {{ t('settings.closePrompt.remember') }}
        </ElCheckbox>
        <div class="close-actions">
          <ElButton :disabled="Boolean(busy)" @click="cancel">{{ t('settings.closePrompt.cancel') }}</ElButton>
          <ElButton :disabled="Boolean(busy)" :loading="busy === 'exit'" plain type="danger"
                    @click="prompt.choose('exit')">{{ t('settings.closePrompt.exit') }}
          </ElButton>
          <ElButton v-if="allowBackground" :disabled="Boolean(busy)" :loading="busy === 'tray'" type="primary"
                    @click="prompt.choose('tray')">
            {{ t(`settings.closePrompt.${closeBackgroundMode === 'window' ? 'window' : 'tray'}`) }}
          </ElButton>
        </div>
      </div>
    </template>
  </ElDialog>
</template>

<style scoped>
.close-description {
  margin: 0;
  color: var(--app-text-secondary);
  line-height: 1.7;
}

.close-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 12px 16px;
}

.close-footer :deep(.el-checkbox) {
  margin-right: 0;
  height: 32px;
}
.close-actions {
  display: flex;
  justify-content: flex-end;
  flex-wrap: wrap;
  gap: 8px;
  margin-left: auto;
}

.close-actions :deep(.el-button + .el-button) {
  margin-left: 0;
}
</style>
