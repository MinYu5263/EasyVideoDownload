<script lang="ts" setup>
import {computed, watch} from "vue";
import {useI18n} from "vue-i18n";
import PlatformSettingsPanel from "./PlatformSettingsPanel.vue";
import type {VideoPlatform} from "../composables/videoPlatforms";
import {usePlatformDirectories} from "../composables/usePlatformDirectories";
import {usePlatformSettings} from "../composables/usePlatformSettings";
import {useCookieSettings} from "../composables/useCookieSettings";
import {useUiPreferences} from "../composables/useUiPreferences";
import {useFeedback} from "../composables/useFeedback";

const {t} = useI18n({useScope: "global"});
const {watchError, notifyError} = useFeedback();
const props = defineProps<{ platform: VideoPlatform }>();
const preferences = useUiPreferences(), directories = usePlatformDirectories(), settings = usePlatformSettings(),
    cookies = useCookieSettings();
const platform = computed(() => props.platform);

async function load(selected = platform.value) {
  await Promise.all([directories.load(selected), settings.load(selected), settings.loadProxy(), cookies.load(selected)]);
}

watch(platform, () => load(), {immediate: true});
watchError(() => settings.loadError[platform.value] || settings.proxyLoadError.value, () => t('download.configuration.loadFailed'), () => {
  const selected = platform.value;
  return {key: `platform-settings:load:${selected}`};
});
watchError(() => cookies.loadError[platform.value], () => t('download.cookie.loadFailed'), () => {
  const selected = platform.value;
  return {key: `cookie:load:${selected}`};
});
watchError(() => cookies.saveError[platform.value], () => t('download.cookie.saveFailed'), () => {
  const selected = platform.value;
  return {key: `cookie:save:${selected}`};
});

async function updateCookie(contents: string) {
  await cookies.update(platform.value, contents);
}

async function updateProxy(enabled: boolean) {
  const selected = platform.value;
  if (await settings.updateProxy(selected, enabled)) return;
  const error = settings.saveError[selected];
  if (error) notifyError(t(error.code === 'proxyNotConfigured' ? 'download.configuration.proxyMissing' : 'download.configuration.saveFailed'), {
    key: `platform-settings:save:${selected}`, detail: error.detail,
  });
}

async function openProxy() {
  await preferences.update({activePage: "settings", settingsSection: "proxy"});
}
</script>

<template>
  <div class="platform-settings-page">
    <PlatformSettingsPanel :key="platform" :contents="cookies.contents[platform]" :cookie-disabled="!cookies.desktop || !cookies.ready[platform]"
                           :directory="directories.settings[platform].directory"
                           :directory-busy="directories.working.value === platform"
                           :directory-disabled="!directories.canChange(platform)"
                           :platform="platform"
                           :proxy="settings.proxy.value" :proxy-enabled="settings.settings[platform].proxyEnabled"
                           :saving="settings.saving[platform]"
                           :settings-disabled="!settings.desktop || !settings.ready[platform] || (!settings.proxyReady.value && !settings.settings[platform].proxyEnabled)"
                           @change="updateCookie"
                           @choose-directory="directories.choose(platform)"
                           @reset-directory="directories.reset(platform)" @proxy-change="updateProxy" @open-proxy="openProxy"/>
  </div>
</template>

<style scoped>
.platform-settings-page {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: min-content;
}
</style>
