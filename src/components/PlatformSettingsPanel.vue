<script lang="ts" setup>
import {computed} from "vue";
import {ElAlert, ElButton, ElSwitch} from "element-plus";
import {Close} from "@element-plus/icons-vue";
import {useI18n} from "vue-i18n";
import CookieImportPanel from "./CookieImportPanel.vue";
import type {VideoPlatform} from "../composables/videoPlatforms";
import type {ProxySettings} from "../composables/useProxySettings";
import type {PersistenceError} from "../composables/useUiPreferences";

const props = defineProps<{
  platform: VideoPlatform; contents: string; cookieDisabled: boolean;
  proxyEnabled: boolean; proxy: ProxySettings | null; settingsDisabled: boolean; saving: boolean;
  loadError: PersistenceError | null; saveError: PersistenceError | null;
}>();
const emit = defineEmits<{
  close: [];
  change: [contents: string];
  proxyChange: [enabled: boolean];
  openProxy: [];
  retry: []
}>();
const {t} = useI18n({useScope: "global"});
const proxyAddress = computed(() => {
  if (!props.proxy) return "";
  const {protocol, address, port} = props.proxy;
  return `${protocol === "socks5" ? "socks5h" : protocol}://${address.includes(":") ? `[${address}]` : address}:${port}`;
});
</script>

<template>
  <section aria-labelledby="platform-settings-title" class="platform-settings">
    <header class="configuration-heading">
      <h2 id="platform-settings-title" tabindex="-1">
        {{ t('download.configuration.title', {platform: t(`download.platforms.${platform}`)}) }}</h2>
      <ElButton :aria-label="t('download.configuration.close')" :icon="Close" class="close-button"
                @click="emit('close')"/>
    </header>
    <section aria-labelledby="platform-proxy-title" class="proxy-section">
      <div class="proxy-row">
        <h3 id="platform-proxy-title">{{ t('download.configuration.useProxy') }}</h3>
        <ElSwitch :aria-label="t('download.configuration.useProxy')" :disabled="settingsDisabled || saving || (!proxy && !proxyEnabled)"
                  :loading="saving" :model-value="proxyEnabled"
                  @update:model-value="emit('proxyChange', Boolean($event))"/>
      </div>
      <div class="proxy-description">
        <span v-if="proxy" class="proxy-address">{{ proxyAddress }}</span>
        <span v-else>{{ t('download.configuration.proxyMissing') }}</span>
        <ElButton link type="primary" @click="emit('openProxy')">{{ t('download.configuration.openProxy') }}</ElButton>
      </div>
      <ElAlert v-if="loadError" :closable="false" :title="t('download.configuration.loadFailed')" show-icon
               type="error">
        <ElButton link type="primary" @click="emit('retry')">{{ t('persistence.retry') }}</ElButton>
      </ElAlert>
      <ElAlert v-if="saveError" :closable="false" :title="t(saveError.code === 'proxyNotConfigured' ? 'download.configuration.proxyMissing' : 'download.configuration.saveFailed')" show-icon
               type="error"/>
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
  padding: 24px;
  border: 1px solid var(--app-border);
  border-radius: var(--app-radius);
  background: var(--app-surface);
}

.configuration-heading, .proxy-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.configuration-heading {
  margin-bottom: 24px;
}

.configuration-heading h2 {
  margin: 0;
  font-size: 18px;
  font-weight: 600;
}

h3 {
  margin: 0;
  font-size: 14px;
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

.proxy-section :deep(.el-alert) {
  margin-top: 12px;
}

.cookie-section {
  margin-top: 22px;
  padding-top: 22px;
  border-top: 1px solid var(--app-border);
}

.cookie-section h3 {
  margin-bottom: 14px;
}

@media (max-width: 800px) {
  .platform-settings {
    padding: 20px;
  }
}
</style>
