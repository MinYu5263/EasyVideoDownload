<script setup lang="ts">
import {ElButton, ElIcon} from "element-plus";
import {Download, InfoFilled, Refresh} from "@element-plus/icons-vue";
import {useI18n} from "vue-i18n";
import {version} from "../../package.json";
import LanguageSettings from "./LanguageSettings.vue";
import ToolSettingsCard from "./ToolSettingsCard.vue";

const {t} = useI18n({useScope: "global"});
const toolIds = ["ytdlp", "ffmpeg", "runtime"] as const;
const toolLinks = [
  {name: "yt-dlp", url: "https://github.com/yt-dlp/yt-dlp"},
  {name: "FFmpeg", url: "https://ffmpeg.org/"},
  {name: "Deno", url: "https://deno.com/"},
  {name: "Node.js", url: "https://nodejs.org/"},
];
</script>

<template>
  <div class="settings-page">
    <LanguageSettings/>

    <section class="tools-section" aria-labelledby="required-tools-title">
      <header class="section-heading">
        <div>
          <h2 id="required-tools-title">{{ t("settings.tools.title") }}</h2>
          <p>{{ t("settings.tools.description") }}</p>
        </div>
        <ElButton :icon="Refresh" disabled :title="t('settings.tools.previewNotice')">
          {{ t("settings.tools.detectAll") }}
        </ElButton>
      </header>

      <div class="tool-list">
        <ToolSettingsCard v-for="toolId in toolIds" :key="toolId" :tool-id="toolId"/>
      </div>

      <p class="tools-notice">
        <ElIcon aria-hidden="true">
          <InfoFilled/>
        </ElIcon>
        <span>{{ t("settings.tools.previewNotice") }}</span>
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
              v-for="link in toolLinks"
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

.tool-list {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.tools-notice {
  display: flex;
  align-items: flex-start;
  gap: 7px;
  margin: 14px 0 0;
  color: var(--app-text-secondary);
  font-size: 11px;
  line-height: 1.7;
}

.tools-notice .el-icon {
  flex-shrink: 0;
  margin-top: 3px;
  color: var(--app-text-muted);
}

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
