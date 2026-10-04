<script lang="ts" setup>
import {onMounted, onUnmounted} from "vue";
import {ElButton, ElCheckbox, ElDialog} from "element-plus";
import {invoke} from "@tauri-apps/api/core";
import {listen} from "@tauri-apps/api/event";
import {useI18n} from "vue-i18n";
import {useAppSettings} from "../composables/useAppSettings";
import {createClosePrompt} from "../composables/useClosePrompt";

const {t} = useI18n({useScope: "global"});
const settings = useAppSettings();
const prompt = createClosePrompt({
  desktop: settings.desktop, invoke,
  listen: handler => listen("app-close-requested", handler)
}, settings);
const {open, remember, busy, error} = prompt;
onMounted(prompt.start);
onUnmounted(prompt.dispose);

function cancel() {
  void prompt.choose("cancel");
}
</script>

<template>
  <div v-if="error && !open" class="persistence-error" role="alert">
    {{ t(`settings.closePrompt.${error.code}`) }}
    <ElButton size="small" @click="prompt.start">{{ t('persistence.retry') }}</ElButton>
  </div>
  <ElDialog :before-close="cancel" :close-on-click-modal="false" :close-on-press-escape="!busy" :model-value="open"
            :show-close="!busy" :title="t('settings.closePrompt.title')"
            align-center append-to-body width="min(500px, calc(100vw - 40px))">
    <p class="close-description">{{ t('settings.closePrompt.description') }}</p>
    <ElCheckbox v-model="remember" :disabled="Boolean(busy) || !settings.ready.value">
      {{ t('settings.closePrompt.remember') }}
    </ElCheckbox>
    <p class="close-hint">{{ t('settings.closePrompt.hint') }}</p>
    <div v-if="error" class="close-error" role="alert">
      <p>{{ t(`settings.closePrompt.${error.code}`) }}</p>
      <details v-if="error.detail">
        <summary>{{ t('settings.requiredTools.errorDetails') }}</summary>
        <p>{{ error.detail }}</p>
      </details>
    </div>
    <template #footer>
      <div class="close-actions">
        <ElButton :disabled="Boolean(busy)" @click="cancel">{{ t('settings.closePrompt.cancel') }}</ElButton>
        <ElButton :disabled="Boolean(busy)" :loading="busy === 'exit'" plain type="danger"
                  @click="prompt.choose('exit')">{{ t('settings.closePrompt.exit') }}
        </ElButton>
        <ElButton :disabled="Boolean(busy)" :loading="busy === 'tray'" type="primary"
                  @click="prompt.choose('tray')">{{ t('settings.closePrompt.tray') }}
        </ElButton>
      </div>
    </template>
  </ElDialog>
</template>

<style scoped>
.close-description {
  margin: 0 0 18px;
  color: var(--app-text-secondary);
  line-height: 1.8;
}

.close-hint {
  margin: 4px 0 0;
  color: var(--app-text-muted);
  font-size: 12px;
  line-height: 1.7;
}

.close-error {
  color: var(--app-danger);
  font-size: 12px;
  line-height: 1.7;
  overflow-wrap: anywhere;
}

.close-error p {
  margin: 12px 0 0;
}

.close-error summary {
  cursor: pointer;
}

.close-actions {
  display: flex;
  justify-content: flex-end;
  flex-wrap: wrap;
  gap: 8px;
}

.close-actions :deep(.el-button + .el-button) {
  margin-left: 0;
}
</style>
