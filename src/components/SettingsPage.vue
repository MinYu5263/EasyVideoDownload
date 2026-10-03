<script setup lang="ts">
import {computed, nextTick, ref, watch} from "vue";
import {ElButton, ElScrollbar, type ScrollbarInstance} from "element-plus";
import {Refresh} from "@element-plus/icons-vue";
import {useI18n} from "vue-i18n";
import ApplicationSettings from "./ApplicationSettings.vue";
import RequiredToolCard from "./RequiredToolCard.vue";
import SegmentedToolbar from "./SegmentedToolbar.vue";
import {toolIds, useRequiredTools} from "../composables/useRequiredTools";
import appIcon from "../assets/app-icon.svg?no-inline";

const {t} = useI18n({useScope: "global"});
const {desktop, ready, loadError, tools, busy, check, choose, changeSource, checkAll} = useRequiredTools();
const sections = [
  {value: "application", labelKey: "settings.application"},
  {value: "tools", labelKey: "settings.requiredTools.title"},
  {value: "proxy", labelKey: "settings.proxy.title"},
  {value: "about", labelKey: "settings.about.title"},
] as const;
const activeSection = ref<typeof sections[number]["value"]>("application");
const sectionOptions = computed(() => sections.map(({value, labelKey}) => ({value, label: t(labelKey)})));
const settingsScrollbar = ref<ScrollbarInstance>();

watch(activeSection, async () => {
  await nextTick();
  settingsScrollbar.value?.setScrollTop(0);
});
</script>

<template>
  <div class="settings-page">
    <div class="settings-toolbar">
      <SegmentedToolbar v-model="activeSection" :ariaLabel="t('settings.chooseSection')" :options="sectionOptions"/>
    </div>

    <ElScrollbar ref="settingsScrollbar" :aria-label="t('navigation.settings')" :tabindex="0"
                 class="settings-scrollbar" height="100%" role="region" view-class="settings-content">
      <ApplicationSettings v-show="activeSection === 'application'" id="settings-application-panel"/>

      <section v-show="activeSection === 'tools'" id="settings-tools-panel" aria-labelledby="required-tools-title"
               class="required-tools-section">
        <header class="section-heading">
          <div>
            <h2 id="required-tools-title">{{ t("settings.requiredTools.title") }}</h2>
          </div>
          <ElButton :disabled="!ready || busy" :icon="Refresh" :loading="busy" @click="checkAll">
            {{ t("settings.requiredTools.checkAll") }}
          </ElButton>
        </header>

        <div class="required-tool-list">
          <RequiredToolCard v-for="toolId in toolIds" :key="toolId" :ready="ready" :state="tools[toolId]"
                            :tool-id="toolId" @check="check(toolId)" @choose="choose(toolId)"
                            @source-change="changeSource(toolId, $event)"/>
        </div>

        <p v-if="!desktop" class="required-tool-notice">{{ t('settings.requiredTools.desktopOnly') }}</p>
        <p v-if="loadError" class="load-error" role="alert">
          {{ t(`settings.requiredTools.errors.${loadError.code}`) }}
          <span v-if="loadError.detail">{{ loadError.detail }}</span>
        </p>
      </section>

      <section v-show="activeSection === 'proxy'" id="settings-proxy-panel" aria-labelledby="proxy-settings-title"
               class="proxy-card">
        <h2 id="proxy-settings-title">{{ t("settings.proxy.title") }}</h2>
        <p role="status">{{ t("settings.proxy.pending") }}</p>
      </section>

      <section v-show="activeSection === 'about'" id="settings-about-panel" aria-labelledby="about-title"
               class="about-section">
        <h2 id="about-title">{{ t("settings.about.title") }}</h2>
        <div class="about-card">
          <div class="about-brand">
            <img :src="appIcon" alt="" aria-hidden="true" class="about-icon" height="36" width="36"/>
            <h3>EasyVideoDownload</h3>
          </div>

          <dl class="about-details">
            <div>
              <dt>{{ t("settings.about.version") }}</dt>
              <dd>v{{ version }}</dd>
            </div>
          </dl>
        </div>
      </section>
    </ElScrollbar>
  </div>
</template>

<style scoped>
.settings-page {
  --app-text-secondary: #617568;

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

.section-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
  margin-bottom: 18px;
}

.section-heading h2,
.about-section > h2,
.proxy-card h2 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
}

.section-heading p {
  margin: 7px 0 0;
  color: var(--app-text-secondary);
  font-size: 12px;
  line-height: 1.7;
}

.section-heading :deep(.el-button) {
  flex-shrink: 0;
  font-size: 12px;
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
  color: #a1392e;
  font-size: 12px;
  line-height: 1.7;
  overflow-wrap: anywhere;
}

.load-error span {
  display: block;
}

.about-section > h2 {
  margin-bottom: 18px;
}

.about-card,
.proxy-card {
  padding: 22px;
  border: 1px solid var(--app-border);
  border-radius: var(--app-radius);
  background: var(--app-surface);
}

.proxy-card p {
  margin: 14px 0 0;
  color: var(--app-text-secondary);
  font-size: 12px;
  line-height: 1.7;
}

.about-card {
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

.about-brand h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
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
  .section-heading {
    flex-wrap: wrap;
    gap: 12px;
  }

  .about-card {
    flex-wrap: wrap;
    gap: 16px;
  }
}
</style>
