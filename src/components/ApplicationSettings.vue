<script lang="ts" setup>
import {computed} from "vue";
import {ElOption, ElSelect, ElSwitch} from "element-plus";
import {useI18n} from "vue-i18n";
import {languageOptions} from "../i18n";
import {useAppSettings} from "../composables/useAppSettings";

const {t} = useI18n({useScope: "global"});
const {desktop, draft: settings, ready, loading, saving, loadError, saveError, update} = useAppSettings();
const locked = computed(() => !ready.value);
const error = computed(() => loadError.value ?? saveError.value);
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

    <div aria-describedby="completion-notification-description" aria-labelledby="completion-notification-label" class="setting-row"
         role="group">
      <div class="setting-description">
        <label id="completion-notification-label"
               for="completion-notification">{{ t("settings.notifications.completion") }}</label>
        <p id="completion-notification-description">{{ t("settings.notifications.completionDescription") }}</p>
      </div>
      <ElSwitch id="completion-notification" :aria-label="t('settings.notifications.completion')"
                :disabled="locked || !desktop"
                :model-value="settings.notifyOnCompletion"
                @update:model-value="update({notifyOnCompletion: Boolean($event)})"/>
    </div>

    <div aria-describedby="failure-notification-description" aria-labelledby="failure-notification-label" class="setting-row"
         role="group">
      <div class="setting-description">
        <label id="failure-notification-label" for="failure-notification">{{
            t("settings.notifications.failure")
          }}</label>
        <p id="failure-notification-description">{{ t("settings.notifications.failureDescription") }}</p>
      </div>
      <ElSwitch id="failure-notification" :aria-label="t('settings.notifications.failure')"
                :disabled="locked || !desktop"
                :model-value="settings.notifyOnFailure"
                @update:model-value="update({notifyOnFailure: Boolean($event)})"/>
    </div>

    <div aria-describedby="close-action-description" aria-labelledby="close-action-label" class="setting-row"
         role="group">
      <div class="setting-description">
        <label id="close-action-label" for="close-action">{{ t("settings.closeAction.title") }}</label>
        <p id="close-action-description">{{ t("settings.closeAction.description") }}</p>
      </div>
      <ElSelect id="close-action" :aria-label="t('settings.closeAction.title')" :disabled="locked || !desktop"
                :model-value="settings.closeAction"
                class="setting-select" @update:model-value="changeCloseAction">
        <ElOption v-for="option in closeOptions" :key="option" :label="t(`settings.closeAction.${option}`)"
                  :value="option"/>
      </ElSelect>
    </div>

    <p v-if="!desktop" class="preview-notice">{{ t("settings.persistence.desktopOnly") }}</p>
    <div v-if="error" class="settings-error" role="alert">
      <p>{{ t(`settings.persistence.errors.${error.code}`) }}</p>
      <details v-if="error.detail">
        <summary>{{ t("settings.requiredTools.errorDetails") }}</summary>
        <p>{{ error.detail }}</p>
      </details>
    </div>
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

.setting-description p {
  margin: 6px 0 0;
  color: var(--app-text-secondary);
  font-size: 12px;
  line-height: 1.7;
}

.setting-select {
  width: 180px;
  flex-shrink: 0;
}

.preview-notice {
  margin: 16px 0 0;
  color: var(--app-text-secondary);
  font-size: 12px;
  line-height: 1.7;
}

.settings-error {
  color: var(--app-danger);
  font-size: 12px;
  line-height: 1.7;
  overflow-wrap: anywhere;
}

.settings-error p {
  margin: 8px 0;
}

.settings-error details {
  margin-top: 8px;
}

.settings-error summary {
  cursor: pointer;
}

@media (max-width: 800px) {
  .setting-row {
    flex-wrap: wrap;
    gap: 16px;
  }
}
</style>
