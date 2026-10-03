<script lang="ts" setup>
import {onUnmounted, watch} from "vue";
import {ElButton, ElInput, ElMessage} from "element-plus";
import {Close, CloseBold} from "@element-plus/icons-vue";
import {useI18n} from "vue-i18n";
import {type VideoPlatform} from "../composables/videoPlatforms";
import {createCookieEditor} from "../composables/useCookieEditor";
import {useDesktopActions} from "../composables/useDesktopActions";

const props = defineProps<{ platform: VideoPlatform; contents: string; disabled: boolean }>();
const emit = defineEmits<{ close: []; change: [contents: string] }>();
const {t} = useI18n({useScope: "global"});
const desktop = useDesktopActions();
const editor = createCookieEditor(props.contents, contents => emit("change", contents));
const {text, reading} = editor;
onUnmounted(editor.dispose);
watch(() => props.contents, contents => {
  if (text.value !== contents) editor.restoreText(contents);
});

async function chooseFile() {
  try {
    await editor.importContents(desktop.importCookie);
  } catch {
    ElMessage.error(t("download.cookie.fileReadFailed"));
  }
}

async function pasteContents() {
  try {
    await editor.readClipboard(desktop.readClipboard);
  } catch {
    ElMessage.error(t("download.cookie.clipboardReadFailed"));
  }
}

</script>

<template>
  <section aria-labelledby="cookie-panel-title" class="cookie-panel">
    <header class="cookie-heading">
      <h2 id="cookie-panel-title" tabindex="-1">{{ t(`download.platforms.${platform}`) }}</h2>
      <ElButton :aria-label="t('download.cookie.close')" :icon="Close" class="close-button" @click="emit('close')"/>
    </header>
    <div :aria-label="t('download.cookie.actions')" class="cookie-editor-actions" role="group">
      <ElButton :disabled="disabled || reading || !desktop.desktop" @click="chooseFile">{{
          t("download.cookie.import")
        }}
      </ElButton>
      <ElButton :disabled="disabled || reading || !desktop.desktop" @click="pasteContents">{{
          t("download.cookie.paste")
        }}
      </ElButton>
    </div>
    <ElInput id="cookie-content" :aria-label="t('download.cookie.content')" :clear-icon="CloseBold" :disabled="disabled"
             :model-value="text"
             :placeholder="t('download.cookie.placeholder')" :rows="12"
             autocomplete="off" clearable spellcheck="false"
             type="textarea" @update:model-value="editor.setText"/>
  </section>
</template>

<style scoped>
.cookie-panel {
  padding: 24px;
  border: 1px solid var(--app-border);
  border-radius: var(--app-radius);
  background: var(--app-surface);
}

.cookie-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 22px;
}

.cookie-heading h2 {
  margin: 0;
  font-size: 18px;
  font-weight: 600;
}

.close-button {
  width: 32px;
  height: 32px;
  flex-shrink: 0;
  padding: 0;
  border: 0;
  background: #2e6b5010;
  color: var(--app-text-secondary);
}

.close-button:hover {
  background: #2e6b5020;
  color: var(--app-accent);
}

.close-button:focus-visible {
  outline: 2px solid var(--app-accent);
  outline-offset: 2px;
}

.cookie-editor-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 14px;
}

.cookie-editor-actions .el-button {
  min-width: 64px;
  margin-left: 0;
}

.cookie-panel :deep(textarea) {
  padding: 12px 26px 12px 14px;
  font-family: Consolas, monospace;
  font-size: 12px;
  line-height: 1.7;
}

@media (max-width: 800px) {
  .cookie-panel {
    padding: 20px;
  }
}
</style>
