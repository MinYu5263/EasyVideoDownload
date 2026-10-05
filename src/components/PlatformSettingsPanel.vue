<script lang="ts" setup>
import {computed} from "vue";
import {ElButton, ElInput, ElSwitch, ElTooltip} from "element-plus";
import {FolderOpened, RefreshRight} from "@element-plus/icons-vue";
import {useI18n} from "vue-i18n";
import CookieImportPanel from "./CookieImportPanel.vue";
import type {VideoPlatform} from "../composables/videoPlatforms";
import type {ProxySettings} from "../composables/useProxySettings";

const props = defineProps<{
  platform: VideoPlatform; contents: string; cookieDisabled: boolean;
  proxyEnabled: boolean; proxy: ProxySettings | null; settingsDisabled: boolean; saving: boolean;
  directory: string; directoryDisabled: boolean; directoryBusy: boolean;
}>();
const emit = defineEmits<{
  chooseDirectory: []; resetDirectory: [];
  change: [contents: string];
  proxyChange: [enabled: boolean];
  openProxy: [];
}>();
const {t} = useI18n({useScope: "global"});
const proxyAddress = computed(() => {
  if (!props.proxy) return "";
  const {protocol, address, port} = props.proxy;
  return `${protocol === "socks5" ? "socks5h" : protocol}://${address.includes(":") ? `[${address}]` : address}:${port}`;
});
</script>

<template>
  <section :aria-label="t('download.configuration.title', {platform: t(`download.platforms.${platform}`)})"
           class="platform-settings">
    <section aria-labelledby="platform-directory-title" class="directory-section">
      <h3 id="platform-directory-title">{{ t('download.saveDirectory') }}</h3>
      <div class="directory-row">
        <ElInput :aria-label="t('download.saveDirectory')" :model-value="directory"
                 :placeholder="t('download.directoryPlaceholder')" readonly>
          <template #append>
            <div class="directory-actions">
              <ElTooltip :content="t('download.chooseDirectory')" placement="top">
                <ElButton :aria-label="t('download.chooseDirectory')" :disabled="directoryDisabled" :icon="FolderOpened"
                          :loading="directoryBusy" @click="emit('chooseDirectory')"/>
              </ElTooltip>
              <ElTooltip :content="t('download.resetDirectory')" placement="top">
                <ElButton :aria-label="t('download.resetDirectory')" :disabled="directoryDisabled" :icon="RefreshRight"
                          @click="emit('resetDirectory')"/>
              </ElTooltip>
            </div>
          </template>
        </ElInput>
      </div>
    </section>
    <section :aria-busy="saving" aria-labelledby="platform-proxy-title" class="proxy-section">
      <div class="proxy-row">
        <h3 id="platform-proxy-title">{{ t('download.configuration.useProxy') }}</h3>
        <ElSwitch :aria-label="t('download.configuration.useProxy')" :disabled="settingsDisabled || saving || (!proxy && !proxyEnabled)"
                  :class="{'is-saving': saving && !settingsDisabled}" :model-value="proxyEnabled" class="proxy-switch"
                  @update:model-value="emit('proxyChange', Boolean($event))"/>
      </div>
      <div class="proxy-description">
        <span v-if="proxy" class="proxy-address">{{ proxyAddress }}</span>
        <span v-else>{{ t('download.configuration.proxyMissing') }}</span>
        <ElButton link type="primary" @click="emit('openProxy')">{{ t('download.configuration.openProxy') }}</ElButton>
      </div>
    </section>
    <section aria-labelledby="platform-cookie-title" class="cookie-section">
      <h3 id="platform-cookie-title">{{ t('download.cookie.title') }}</h3>
      <CookieImportPanel :contents="contents" :disabled="cookieDisabled" :platform="platform" embedded
                         @change="emit('change', $event)"/>
    </section>
  </section>
</template>

<style scoped>
.platform-settings {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: min-content;
  padding: 24px;
  border: 1px solid var(--app-border);
  border-radius: var(--app-radius);
  background: var(--app-surface);
}

.proxy-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.proxy-switch.is-saving {
  /* Prevent duplicate toggles without dimming the switch during persistence. */
  opacity: 1;
}

h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
}

.directory-section {
  flex-shrink: 0;
  margin-bottom: 22px;
  padding-bottom: 22px;
  border-bottom: 1px solid var(--app-border);
}

.proxy-section {
  flex-shrink: 0;
}

.directory-row {
  display: flex;
  margin-top: 12px;
}

.directory-row .el-input {
  flex: 1;
  min-width: 0;
}

.directory-row :deep(.el-input-group__append) {
  padding: 0;
  background: var(--app-surface);
}

.directory-row :deep(.el-input-group__append .el-button) {
  margin: 0;
  width: 40px;
  height: 36px;
  padding: 0;
  border: 0;
  border-radius: 0;
}

.directory-actions {
  display: flex;
}

.directory-actions :deep(.el-button + .el-button) {
  border-left: 1px solid var(--app-border);
}

.proxy-description {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px 16px;
  margin-top: 8px;
  font-size: 12px;
  color: var(--app-text-secondary);
}

.proxy-address {
  overflow-wrap: anywhere;
}


.cookie-section {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: min-content;
  margin-top: 22px;
  padding-top: 22px;
  border-top: 1px solid var(--app-border);
}

.cookie-section h3 {
  flex-shrink: 0;
  margin-bottom: 14px;
}


@media (max-width: 800px) {
  .platform-settings {
    padding: 20px;
  }
}
</style>
