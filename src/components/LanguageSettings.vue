<script setup lang="ts">
import {ElOption, ElSelect} from "element-plus";
import {useI18n} from "vue-i18n";
import {appLocale, languageOptions} from "../i18n";

const {t} = useI18n({useScope: "global"});
</script>

<template>
  <section class="settings-card" aria-labelledby="application-settings-title">
    <h2 id="application-settings-title">{{ t("settings.application") }}</h2>
    <div class="setting-row">
      <div class="setting-description">
        <label for="interface-language">{{ t("settings.language") }}</label>
        <p>{{ t("settings.languageDescription") }}</p>
      </div>
      <ElSelect
          id="interface-language"
          v-model="appLocale"
          class="language-select"
          :aria-label="t('settings.language')"
      >
        <ElOption
            v-for="option in languageOptions"
            :key="option.value"
            :value="option.value"
            :label="t(option.labelKey)"
        />
      </ElSelect>
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

.settings-card h2 {
  margin: 0 0 20px;
  color: var(--app-text);
  font-size: 16px;
  font-weight: 600;
}

.setting-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 24px;
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
  color: var(--app-text-muted);
  font-size: 12px;
  line-height: 1.7;
}

.language-select {
  width: 180px;
  flex-shrink: 0;
}

@media (max-width: 800px) {
  .setting-row {
    flex-wrap: wrap;
    gap: 16px;
  }
}
</style>
