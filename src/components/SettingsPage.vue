<script setup lang="ts">
import {ElButton, ElIcon} from "element-plus";
import {Download, Refresh} from "@element-plus/icons-vue";
import {useI18n} from "vue-i18n";
import {version} from "../../package.json";
import LanguageSettings from "./LanguageSettings.vue";
import RequiredToolCard from "./RequiredToolCard.vue";
import {toolIds, useRequiredTools} from "../composables/useRequiredTools";

const {t} = useI18n({useScope: "global"});
const {desktop, ready, loadError, tools, busy, check, choose, changeSource, checkAll} = useRequiredTools();
const requiredToolLinks = [
  {name: "yt-dlp", url: "https://github.com/yt-dlp/yt-dlp"},
  {name: "FFmpeg", url: "https://ffmpeg.org/"},
  {name: "Deno", url: "https://deno.com/"},
];
</script>

<template>
  <div class="settings-page">
    <LanguageSettings/>

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
        <RequiredToolCard v-for="toolId in toolIds" :key="toolId" :tool-id="toolId" :state="tools[toolId]" :ready="ready" @check="check(toolId)" @choose="choose(toolId)" @source-change="changeSource(toolId, $event)"/>
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
          <span class="about-icon" aria-hidden="true">
            <ElIcon :size="19"><Download/></ElIcon>
          </span>
          <div>
            <h3>EasyVideoDownload</h3>
            <p>{{ t("settings.about.tagline") }}</p>
          </div>
        </div>

        <dl class="about-details">
          <div>
            <dt>{{ t("settings.about.version") }}</dt>
            <dd>v{{ version }}</dd>
          </div>
          <div>
            <dt>{{ t("settings.about.platforms") }}</dt>
            <dd>Windows / macOS / Linux</dd>
          </div>
        </dl>

        <nav class="about-links" :aria-label="t('settings.about.links')">
          <a
              v-for="link in requiredToolLinks"
              :key="link.name"
              :href="link.url"
              target="_blank"
              rel="noopener noreferrer"
          >{{ link.name }}</a>
        </nav>
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

.load-error { color: #a1392e; font-size: 12px; line-height: 1.7; overflow-wrap: anywhere; }
.load-error span { display: block; }

.about-section > h2 {
  margin-bottom: 18px;
}

.about-card {
  padding: 22px;
  border: 1px solid var(--app-border);
  border-radius: var(--app-radius);
  background: var(--app-surface);
}

.about-brand {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 22px;
}

.about-icon {
  display: grid;
  width: 36px;
  height: 36px;
  flex-shrink: 0;
  place-items: center;
  border-radius: 11px;
  background: var(--app-accent);
  color: #ffffff;
}

.about-brand h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
}

.about-brand p {
  margin: 4px 0 0;
  color: var(--app-text-secondary);
  font-size: 12px;
}

.about-details {
  margin: 0;
  font-size: 12px;
}

.about-details > div {
  display: flex;
  justify-content: space-between;
  gap: 20px;
  padding: 14px 0;
  border-top: 1px solid var(--app-border);
}

.about-details dt {
  color: var(--app-text-secondary);
}

.about-details dd {
  margin: 0;
  color: var(--app-text-secondary);
  text-align: right;
}

.about-links {
  display: flex;
  flex-wrap: wrap;
  gap: 20px;
  margin-top: 16px;
  font-size: 12px;
}

.about-links a:hover {
  text-decoration: underline;
}

.about-links a:focus-visible {
  outline: 2px solid var(--app-accent);
  outline-offset: 4px;
  border-radius: 2px;
}

@media (max-width: 800px) {
  .section-heading {
    flex-wrap: wrap;
    gap: 12px;
  }

  .about-details > div {
    flex-wrap: wrap;
    gap: 8px;
  }
}
</style>
