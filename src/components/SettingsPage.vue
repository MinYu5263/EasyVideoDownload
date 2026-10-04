<script setup lang="ts">
import {computed, nextTick, ref, watch} from "vue";
import {ElButton, ElMessage, ElScrollbar, type ScrollbarInstance} from "element-plus";
import {FolderOpened} from "@element-plus/icons-vue";
import {invoke} from "@tauri-apps/api/core";
import {useI18n} from "vue-i18n";
import ApplicationSettings from "./ApplicationSettings.vue";
import ProxySettings from "./ProxySettings.vue";
import RequiredToolCard from "./RequiredToolCard.vue";
import SegmentedToolbar from "./SegmentedToolbar.vue";
import {toolIds, useRequiredTools} from "../composables/useRequiredTools";
import appIcon from "../assets/app-icon.svg?no-inline";
import {version} from "../../package.json";
import {useUiPreferences} from "../composables/useUiPreferences";

const {t} = useI18n({useScope: "global"});
const {
  desktop,
  ready,
  loadError,
  automaticSupported,
  automaticFfmpegRequiresRosetta,
  tools,
  load,
  check,
  choose,
  changeSource,
  configure,
  cancelConfiguration
} = useRequiredTools();
const sections = [
  {value: "application", labelKey: "settings.application"},
  {value: "tools", labelKey: "settings.requiredTools.title"},
  {value: "proxy", labelKey: "settings.proxy.title"},
  {value: "about", labelKey: "settings.about.title"},
] as const;
const uiPreferences = useUiPreferences();
const activeSection = computed({
  get: () => uiPreferences.draft.settingsSection, set: value => {
    void uiPreferences.update({settingsSection: value});
  }
});
const sectionOptions = computed(() => sections.map(({value, labelKey}) => ({value, label: t(labelKey)})));
const settingsScrollbar = ref<ScrollbarInstance>();
const openingDataDirectory = ref(false);

async function openDataDirectory() {
  if (!desktop || openingDataDirectory.value) return;
  openingDataDirectory.value = true;
  try {
    await invoke("open_app_data_directory");
  } catch {
    ElMessage.error(t("settings.dataDirectory.openFailed"));
  } finally {
    openingDataDirectory.value = false;
  }
}

watch(activeSection, async () => {
  await nextTick();
  settingsScrollbar.value?.setScrollTop(0);
});
watch(() => uiPreferences.draft.activePage === "settings" && activeSection.value === "tools", visible => {
  if (visible) void load();
});
</script>

<template>
  <div class="settings-page">
    <div class="settings-toolbar">
      <SegmentedToolbar v-model="activeSection" :ariaLabel="t('settings.chooseSection')"
                        :disabled="!uiPreferences.ready.value" :options="sectionOptions"/>
    </div>

    <ElScrollbar ref="settingsScrollbar" :aria-label="t('navigation.settings')" :tabindex="0"
                 class="settings-scrollbar" height="100%" role="region" view-class="settings-content">
      <ApplicationSettings v-show="activeSection === 'application'" id="settings-application-panel"/>

      <section v-show="activeSection === 'tools'" id="settings-tools-panel"
               :aria-label="t('settings.requiredTools.title')"
               class="required-tools-section">
        <div class="required-tool-list">
          <RequiredToolCard v-for="toolId in toolIds" :key="toolId" :ready="ready" :state="tools[toolId]"
                            :automatic-supported="automaticSupported[toolId]"
                            :requires-rosetta="toolId === 'ffmpeg' && automaticFfmpegRequiresRosetta"
                            :tool-id="toolId" @check="check(toolId)" @choose="choose(toolId)"
                            @cancel="cancelConfiguration(toolId)" @configure="configure(toolId)"
                            @source-change="changeSource(toolId, $event)"/>
        </div>

        <p v-if="!desktop" class="required-tool-notice">{{ t('settings.requiredTools.desktopOnly') }}</p>
        <p v-if="loadError" class="load-error" role="alert">
          {{ t(`settings.requiredTools.errors.${loadError.code}`) }}
          <span v-if="loadError.detail">{{ loadError.detail }}</span>
        </p>
      </section>

      <ProxySettings v-show="activeSection === 'proxy'" id="settings-proxy-panel"/>

      <section v-show="activeSection === 'about'" id="settings-about-panel" :aria-label="t('settings.about.title')"
               class="about-section">
        <div class="about-card">
          <div class="about-brand">
            <img :src="appIcon" alt="" aria-hidden="true" class="about-icon" height="36" width="36"/>
            <div class="about-brand-text">
              <h2>EasyVideoDownload</h2>
              <p>{{ t("settings.about.description") }}</p>
            </div>
          </div>

          <dl class="about-details">
            <div>
              <dt>{{ t("settings.about.version") }}</dt>
              <dd>v{{ version }}</dd>
            </div>
          </dl>
        </div>
        <div aria-describedby="data-directory-description" aria-labelledby="data-directory-label" class="data-directory-card"
             role="group">
          <div>
            <h3 id="data-directory-label">{{ t("settings.dataDirectory.title") }}</h3>
            <p id="data-directory-description">{{ t("settings.dataDirectory.description") }}</p>
          </div>
          <ElButton :disabled="!desktop || openingDataDirectory" :icon="FolderOpened" :loading="openingDataDirectory"
                    @click="openDataDirectory">
            {{ t("settings.dataDirectory.open") }}
          </ElButton>
        </div>
      </section>
    </ElScrollbar>
  </div>
</template>

<style scoped>
.settings-page {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
}

.settings-toolbar {
  flex-shrink: 0;
  margin: 0 var(--app-page-padding-x) 18px;
}

.settings-scrollbar {
  flex: 1;
  min-height: 0;
}

.settings-scrollbar :deep(.settings-content) {
  padding: 0 var(--app-page-padding-x) var(--app-page-padding-bottom);
}

.required-tool-list {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.required-tool-notice {
  display: flex;
  align-items: flex-start;
  gap: 7px;
  margin: 14px 0 0;
  color: var(--app-text-secondary);
  font-size: 11px;
  line-height: 1.7;
}

.load-error {
  color: var(--app-danger);
  font-size: 12px;
  line-height: 1.7;
  overflow-wrap: anywhere;
}

.load-error span {
  display: block;
}

.about-card,
.data-directory-card {
  padding: 22px;
  border: 1px solid var(--app-border);
  border-radius: var(--app-radius);
  background: var(--app-surface);
}

.about-card,
.data-directory-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
}

.about-brand {
  display: flex;
  align-items: center;
  gap: 12px;
}

.about-icon {
  display: block;
  width: 36px;
  height: 36px;
  flex-shrink: 0;
}

.about-brand-text h2 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
}

.about-section {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.data-directory-card h3 {
  margin: 0;
  color: var(--app-text);
  font-size: 13px;
  font-weight: 500;
}

.data-directory-card p {
  margin: 6px 0 0;
  color: var(--app-text-secondary);
  font-size: 12px;
  line-height: 1.7;
}

.data-directory-card :deep(.el-button) {
  flex-shrink: 0;
  font-size: 12px;
}

.about-brand-text p {
  margin: 4px 0 0;
  color: var(--app-text-secondary);
  font-size: 12px;
  line-height: 1.5;
}

.about-details {
  margin: 0;
  font-size: 12px;
}

.about-details > div {
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.about-details dt {
  color: var(--app-text-secondary);
}

.about-details dd {
  margin: 0;
  color: var(--app-text-secondary);
  text-align: right;
}

@media (max-width: 800px) {
  .about-card,
  .data-directory-card {
    flex-wrap: wrap;
    gap: 16px;
  }
}
</style>
