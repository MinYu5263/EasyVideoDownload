<script lang="ts" setup>
import {onUnmounted, watch} from "vue";
import {ElButton, ElInput} from "element-plus";
import {Close, CloseBold} from "@element-plus/icons-vue";
import {useI18n} from "vue-i18n";
import {type VideoPlatform} from "../composables/videoPlatforms";
import {createCookieEditor} from "../composables/useCookieEditor";
import {useDesktopActions} from "../composables/useDesktopActions";
import {useFeedback} from "../composables/useFeedback";
import {persistenceError} from "../composables/useUiPreferences";

const props = defineProps<{ platform: VideoPlatform; contents: string; disabled: boolean; embedded?: boolean }>();
const emit = defineEmits<{ close: []; change: [contents: string] }>();
const {t} = useI18n({useScope: "global"});
const {notifyError} = useFeedback();
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
  } catch (error) {
    notifyError(t("download.cookie.fileReadFailed"), {detail: persistenceError(error).detail});
  }
}

async function pasteContents() {
  try {
    await editor.readClipboard(desktop.readClipboard);
  } catch (error) {
    notifyError(t("download.cookie.clipboardReadFailed"), {detail: persistenceError(error).detail});
  }
}

</script>

<template>
  <section :aria-labelledby="embedded ? 'platform-cookie-title' : 'cookie-panel-title'" :class="{embedded}"
           class="cookie-panel">
    <header v-if="!embedded" class="cookie-heading">
      <h2 id="cookie-panel-title" tabindex="-1">{{ t(`download.platforms.${platform}`) }}</h2>
      <ElButton :aria-label="t('download.cookie.close')" :icon="Close" class="close-button" @click="emit('close')"/>
    </header>
    <div :class="{'is-disabled': disabled}" class="cookie-editor">
      <ElInput id="cookie-content" :aria-label="t('download.cookie.content')" :clear-icon="CloseBold"
               :disabled="disabled"
               :model-value="text" :placeholder="t('download.cookie.placeholder')"
               autocomplete="off"
               class="cookie-input" clearable resize="none" spellcheck="false"
               type="textarea" @update:model-value="editor.setText"/>
      <div :aria-label="t('download.cookie.actions')" class="cookie-editor-actions" role="group">
        <ElButton :disabled="disabled || reading || !desktop.desktop" size="small" @click="chooseFile">{{
            t("download.cookie.import")
          }}
        </ElButton>
        <ElButton :disabled="disabled || reading || !desktop.desktop" size="small" @click="pasteContents">{{
            t("download.cookie.paste")
          }}
        </ElButton>
      </div>
    </div>
  </section>
</template>

<style scoped>
.cookie-panel {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
  padding: 24px;
  border: 1px solid var(--app-border);
  border-radius: var(--app-radius);
  background: var(--app-surface);
}

.cookie-panel.embedded {
  padding: 0;
  border: 0;
  border-radius: 0;
}

.cookie-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 22px;
  flex-shrink: 0;
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
  background: var(--app-accent-soft);
  color: var(--app-text-secondary);
}

.close-button:hover {
  background: var(--app-hover);
  color: var(--app-accent);
}

.close-button:focus-visible {
  outline: 2px solid var(--app-accent);
  outline-offset: 2px;
}

.cookie-editor {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 204px;
  border: 1px solid var(--el-border-color);
  border-radius: var(--el-border-radius-base);
  background: var(--el-fill-color-blank);
  overflow: hidden;
}

.cookie-editor:hover {
  border-color: var(--el-border-color-hover);
}

.cookie-editor:focus-within {
  border-color: var(--el-color-primary);
}

.cookie-editor.is-disabled {
  border-color: var(--el-disabled-border-color);
  background: var(--el-disabled-bg-color);
}

.cookie-input {
  display: flex;
  flex: 1;
  min-height: 160px;
}

.cookie-editor-actions {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  gap: 10px;
  padding: 8px 12px;
  border-top: 1px solid var(--app-border);
}

.cookie-editor-actions .el-button {
  min-width: 64px;
  margin-left: 0;
}

.cookie-input :deep(textarea) {
  height: 100%;
  min-height: 160px;
  padding: 12px 32px 12px 14px;
  border-radius: 0;
  box-shadow: none;
  font-family: Consolas, monospace;
  font-size: 12px;
  line-height: 1.7;
}

@media (max-width: 800px) {
  .cookie-panel {
    padding: 20px;
  }

  .cookie-panel.embedded {
    padding: 0;
  }
}
</style>
