<script lang="ts" setup>
import {computed} from "vue";
import {ElInputNumber, ElOption, ElSelect, ElSwitch} from "element-plus";
import {useI18n} from "vue-i18n";
import {languageOptions} from "../i18n";
import {useAppSettings} from "../composables/useAppSettings";

const {t} = useI18n({useScope: "global"});
const {desktop, draft: settings, closeBackgroundMode, ready, loading, saving, update} = useAppSettings();
const locked = computed(() => !ready.value);
const themeOptions = ["system", "light", "dark"] as const;
const closeOptions = ["ask", "tray", "exit"] as const;

function changeLocale(value: unknown) {
  if (value === "zh-CN" || value === "en") void update({locale: value});
}

function changeTheme(value: unknown) {
  if (value === "system" || value === "light" || value === "dark") void update({theme: value});
}

function changeCloseAction(value: unknown) {
  if (value === "ask" || value === "tray" || value === "exit") void update({closeAction: value});
}

function changeDownloadLimit(value: unknown) {
  if (typeof value === "number" && Number.isInteger(value) && value >= 1 && value <= 6) {
    void update({maxConcurrentDownloads: value});
  }
}
</script>

<template>
  <section :aria-busy="loading || saving" :aria-label="t('settings.application')" class="settings-card">

    <div aria-describedby="interface-language-description" aria-labelledby="interface-language-label" class="setting-row"
         role="group">
      <div class="setting-description">
        <label id="interface-language-label" for="interface-language">{{ t("settings.language") }}</label>
        <p id="interface-language-description">{{ t("settings.languageDescription") }}</p>
      </div>
      <ElSelect id="interface-language" :aria-label="t('settings.language')" :disabled="locked" :model-value="settings.locale"
                class="setting-select" @update:model-value="changeLocale">
        <ElOption v-for="option in languageOptions" :key="option.value" :label="t(option.labelKey)"
                  :value="option.value"/>
      </ElSelect>
    </div>

    <div aria-describedby="interface-theme-description" aria-labelledby="interface-theme-label" class="setting-row"
         role="group">
      <div class="setting-description">
        <label id="interface-theme-label" for="interface-theme">{{ t("settings.theme.title") }}</label>
        <p id="interface-theme-description">{{ t("settings.theme.description") }}</p>
      </div>
      <ElSelect id="interface-theme" :aria-label="t('settings.theme.title')" :disabled="locked" :model-value="settings.theme"
                class="setting-select" @update:model-value="changeTheme">
        <ElOption v-for="option in themeOptions" :key="option" :label="t(`settings.theme.${option}`)" :value="option"/>
      </ElSelect>
    </div>

    <div aria-describedby="download-limit-description" aria-labelledby="download-limit-label" class="setting-row"
         role="group">
      <div class="setting-description">
        <label id="download-limit-label" for="download-limit">{{ t("settings.downloadLimit.title") }}</label>
        <p id="download-limit-description">{{ t("settings.downloadLimit.description") }}</p>
      </div>
      <ElInputNumber id="download-limit" :aria-label="t('settings.downloadLimit.title')"
                     :disabled="locked || !desktop" :max="6"
                     :min="1" :model-value="settings.maxConcurrentDownloads" :precision="0" :step="1"
                     :value-on-clear="settings.maxConcurrentDownloads" aria-describedby="download-limit-description" class="setting-select"
                     step-strictly @update:model-value="changeDownloadLimit"/>
    </div>

    <div aria-describedby="completion-notification-description" aria-labelledby="completion-notification-label" class="setting-row"
         role="group">
      <div class="setting-description">
        <label id="completion-notification-label"
               for="completion-notification">{{ t("settings.notifications.completion") }}</label>
        <p id="completion-notification-description">{{ t("settings.notifications.completionDescription") }}</p>
      </div>
      <ElSwitch id="completion-notification" :aria-label="t('settings.notifications.completion')"
                :disabled="locked"
                :model-value="settings.notifyOnCompletion"
                @update:model-value="update({notifyOnCompletion: Boolean($event)})"/>
    </div>

    <div aria-describedby="close-action-description" aria-labelledby="close-action-label" class="setting-row"
         role="group">
      <div class="setting-description">
        <label id="close-action-label" for="close-action">{{ t("settings.closeAction.title") }}</label>
        <p id="close-action-description">{{ t("settings.closeAction.description") }}</p>
      </div>
      <ElSelect id="close-action" :aria-label="t('settings.closeAction.title')" :disabled="locked || !desktop"
                :model-value="settings.closeAction"
                class="setting-select close-behavior-select" @update:model-value="changeCloseAction">
        <ElOption v-for="option in closeOptions" :key="option"
                  :label="t(`settings.closeAction.${option === 'tray' && closeBackgroundMode === 'window' ? 'window' : option}`)"
                  :value="option"/>
      </ElSelect>
    </div>

    <p v-if="!desktop" class="preview-notice">{{ t("settings.persistence.desktopOnly") }}</p>
  </section>
</template>

<style scoped>
.settings-card {
  padding: 22px;
  border: 1px solid var(--app-border);
  border-radius: var(--app-radius);
  background: var(--app-surface);
}

.setting-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 24px;
  padding: 17px 0;
}

.setting-row + .setting-row {
  border-top: 1px solid var(--app-border);
}

.setting-row:first-child {
  padding-top: 0;
}

.setting-row:last-of-type {
  padding-bottom: 0;
}

.setting-description {
  min-width: 0;
}

.setting-description label {
  color: var(--app-text);
  font-size: 13px;
  font-weight: 500;
}

.setting-select {
  width: 180px;
  flex-shrink: 0;
}

.close-behavior-select {
  width: 280px;
  max-width: 100%;
}

.setting-description p {
  margin: 6px 0 0;
  color: var(--app-text-secondary);
  font-size: 12px;
  line-height: 1.7;
}

.preview-notice {
  margin: 16px 0 0;
  color: var(--app-text-secondary);
  font-size: 12px;
  line-height: 1.7;
}


@media (max-width: 800px) {
  .setting-row {
    flex-wrap: wrap;
    gap: 16px;
  }
}
</style>
