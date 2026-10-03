<script lang="ts" setup>
import {computed, onUnmounted, watch} from "vue";
import {ElButton, ElDialog, ElInput, ElMessage, ElTooltip} from "element-plus";
import {invoke} from "@tauri-apps/api/core";
import {useI18n} from "vue-i18n";
import {type VideoPlatform} from "../composables/videoPlatforms";
import {createVideoCommand, type DownloadCommandOptions} from "../composables/useVideoCommand";
import {useCookieSettings} from "../composables/useCookieSettings";
import {useDesktopActions} from "../composables/useDesktopActions";

const props = defineProps<{
  title: string; platform: VideoPlatform; input: string; active: boolean; disabled: boolean;
  downloadOptions?: DownloadCommandOptions;
}>();
const {t} = useI18n({useScope: "global"});
const desktop = useDesktopActions();
const cookies = useCookieSettings();
const viewer = createVideoCommand({
  desktop: desktop.desktop,
  invoke,
  beforeRead: cookies.whenIdle,
  writeClipboard: desktop.writeClipboard
});
const {open, command, loading, copying, error} = viewer;
const errorMessage = computed(() => {
  const known = ["invalidLink", "platformMismatch", "desktopOnly", "cookieSaveFailed", "cookieReadFailed", "toolMissing",
    "toolSettingsFailed", "commandUnavailable", "clipboardWriteFailed", "invalidDownloadDirectory", "invalidDownloadOptions", "ffmpegMissing"];
  const code = error.value && known.includes(error.value.code) ? error.value.code : "bridgeFailed";
  return t(`download.errors.${code}`, {platform: t(`download.platforms.${props.platform}`)});
});
watch(() => [props.active, props.disabled] as const, ([active, disabled]) => {
  if (!active || disabled) viewer.close();
});
watch(() => [props.platform, props.input, props.downloadOptions] as const, () => viewer.close());
onUnmounted(viewer.dispose);

function setOpen(value: boolean) {
  if (!value) viewer.close();
}

async function copy() {
  if (await viewer.copy()) ElMessage.success(t("download.command.copied"));
}
</script>

<template>
  <span class="command-viewer">
    <ElTooltip :content="t('download.command.viewNamed', {title})" :show-after="200" placement="top">
      <span class="command-trigger-wrap">
        <ElButton :aria-expanded="open" :aria-label="t('download.command.viewNamed', {title})" :disabled="disabled"
                  class="command-trigger" native-type="button"
                  @click="viewer.show(platform, input, downloadOptions)">
          <svg aria-hidden="true" fill="none" height="18" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"
               stroke-width="1.7" viewBox="0 0 24 24" width="18">
            <rect height="16" rx="3" width="18" x="3" y="4"/>
            <path d="m7 8 3 3-3 3m6 0h4"/>
          </svg>
        </ElButton>
      </span>
    </ElTooltip>
    <ElDialog :model-value="open" :title="title" align-center
              append-to-body width="min(720px, calc(100vw - 40px))" @update:model-value="setOpen">
      <p v-if="loading" class="command-note" role="status">{{ t('download.command.loading') }}</p>
      <template v-if="command">
        <p class="command-note">{{
            t(downloadOptions ? 'download.command.downloadDescription' : 'download.command.description', {shell: command.shell})
          }}</p>
        <ElInput :aria-label="title" :model-value="command.text" :rows="8" class="command-text" readonly
                 spellcheck="false" type="textarea"/>
      </template>
      <p v-if="error" class="command-error" role="alert">{{ errorMessage }}</p>
      <template #footer>
        <ElButton @click="viewer.close">{{ t('download.command.close') }}</ElButton>
        <ElButton :disabled="!command || copying" :loading="copying" type="primary"
                  @click="copy">{{ t('download.command.copy') }}</ElButton>
      </template>
    </ElDialog>
  </span>
</template>

<style scoped>
.command-viewer, .command-trigger-wrap {
  display: inline-flex;
  flex-shrink: 0;
}

.command-trigger {
  width: 32px;
  height: 32px;
  padding: 0;
  border: 1px solid var(--app-border);
  border-radius: 7px;
  background: var(--app-background);
  color: var(--app-text-secondary);
}

.command-trigger:hover:not(:disabled) {
  border-color: var(--app-accent);
  color: var(--app-accent);
  background: var(--app-accent-soft);
}

.command-trigger:focus-visible {
  outline: 2px solid var(--app-accent);
  outline-offset: 2px;
}

.command-note {
  margin: 0 0 14px;
  color: var(--app-text-secondary);
  font-size: 12px;
  line-height: 1.7;
}

.command-text :deep(textarea) {
  padding: 12px 14px;
  font-family: Consolas, monospace;
  font-size: 12px;
  line-height: 1.7;
}

.command-error {
  margin: 12px 0 0;
  color: #a1392e;
  font-size: 12px;
  line-height: 1.7;
}
</style>
