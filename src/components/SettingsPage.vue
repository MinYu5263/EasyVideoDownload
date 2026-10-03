<script setup lang="ts">
import {ElButton} from "element-plus";
import {Refresh} from "@element-plus/icons-vue";
import {useI18n} from "vue-i18n";
import ApplicationSettings from "./ApplicationSettings.vue";
import RequiredToolCard from "./RequiredToolCard.vue";
import {toolIds, useRequiredTools} from "../composables/useRequiredTools";
import appIcon from "../assets/app-icon.svg?no-inline";

const {t} = useI18n({useScope: "global"});
const {desktop, ready, loadError, tools, busy, check, choose, changeSource, checkAll} = useRequiredTools();
</script>

<template>
  <div class="settings-page">
    <ApplicationSettings/>

    <section class="required-tools-section" aria-labelledby="required-tools-title">
      <header class="section-heading">
        <div>
          <h2 id="required-tools-title">{{ t("settings.requiredTools.title") }}</h2>
        </div>
        <ElButton :icon="Refresh" :disabled="!ready || busy" :loading="busy" @click="checkAll">
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

    <section class="about-section" aria-labelledby="about-title">
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
  </div>
</template>

<style scoped>
.settings-page {
  --app-text-secondary: #617568;

  display: flex;
  flex-direction: column;
  gap: 30px;
}

.section-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
  margin-bottom: 18px;
}

.section-heading h2,
.about-section > h2 {
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

.about-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
  padding: 22px;
  border: 1px solid var(--app-border);
  border-radius: var(--app-radius);
  background: var(--app-surface);
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
