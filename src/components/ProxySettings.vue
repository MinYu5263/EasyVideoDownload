<script lang="ts" setup>
import {computed} from "vue";
import {ElAlert, ElButton, ElInput, ElOption, ElSelect} from "element-plus";
import {useI18n} from "vue-i18n";
import {useProxySettings} from "../composables/useProxySettings";

const {t} = useI18n({useScope: "global"});
const protocols = [
  {value: "http", label: "HTTP"},
  {value: "https", label: "HTTPS"},
  {value: "socks5", label: "SOCKS5"},
] as const;
const {
  desktop, draft, saved, ready, loading, saving, testing, dirty, addressInvalid, portInvalid, canSave, canTest,
  proxyAddress, loadError, saveError, testError, testResult, save, testConnection, load
} = useProxySettings();
const failures = computed(() => [
  {error: loadError.value, title: "settings.proxy.loadFailedTitle"},
  {error: saveError.value, title: "settings.proxy.saveFailedTitle"},
  {error: testError.value, title: "settings.proxy.testFailedTitle"},
].flatMap(item => item.error ? [{error: item.error, title: item.title}] : []));

function errorMessage(code: string) {
  const known = ["loadFailed", "saveFailed", "invalidSettings", "bridgeFailed", "timeout", "connectionRefused",
    "proxyAuthRequired", "tlsFailed", "proxyConnectionFailed", "requestFailed", "targetHttpError", "responseTooLarge", "testUnavailable"];
  return t(`settings.proxy.errors.${known.includes(code) ? code : "bridgeFailed"}`);
}
</script>

<template>
  <section :aria-busy="loading || saving || testing" :aria-label="t('settings.proxy.title')"
           class="proxy-settings-card">
    <div class="proxy-fields">
      <div class="proxy-field">
        <label for="proxy-protocol">{{ t("settings.proxy.protocol") }}</label>
        <ElSelect id="proxy-protocol" v-model="draft.protocol" :aria-label="t('settings.proxy.protocol')"
                  :disabled="!ready"
                  aria-describedby="proxy-protocol-hint">
          <ElOption v-for="protocol in protocols" :key="protocol.value" :label="protocol.label"
                    :value="protocol.value"/>
        </ElSelect>
        <p id="proxy-protocol-hint" class="field-hint">
          {{ t(draft.protocol === 'socks5' ? "settings.proxy.socksHint" : "settings.proxy.protocolHint") }}
        </p>
      </div>

      <div :class="{'is-invalid': addressInvalid}" class="proxy-field">
        <label for="proxy-address">{{ t("settings.proxy.address") }}</label>
        <ElInput id="proxy-address" v-model="draft.address" :aria-invalid="addressInvalid" :aria-label="t('settings.proxy.address')"
                 :disabled="!ready"
                 :placeholder="t('settings.proxy.addressPlaceholder')" aria-describedby="proxy-address-hint"
                 autocomplete="off" clearable spellcheck="false"/>
        <p id="proxy-address-hint" :role="addressInvalid ? 'alert' : undefined" class="field-hint">
          {{ t(addressInvalid ? "settings.proxy.addressInvalid" : "settings.proxy.addressHint") }}
        </p>
      </div>

      <div :class="{'is-invalid': portInvalid}" class="proxy-field">
        <label for="proxy-port">{{ t("settings.proxy.port") }}</label>
        <ElInput id="proxy-port" v-model="draft.port" :aria-invalid="portInvalid" :aria-label="t('settings.proxy.port')"
                 :disabled="!ready"
                 :placeholder="t('settings.proxy.portPlaceholder')" aria-describedby="proxy-port-hint"
                 autocomplete="off" inputmode="numeric" maxlength="5"/>
        <p id="proxy-port-hint" :role="portInvalid ? 'alert' : undefined" class="field-hint">
          {{ t(portInvalid ? "settings.proxy.portInvalid" : "settings.proxy.portHint") }}
        </p>
      </div>
    </div>

    <div class="proxy-preview">
      <span id="proxy-preview-label">{{ t("settings.proxy.addressPreview") }}</span>
      <div aria-labelledby="proxy-preview-label" aria-live="polite" class="proxy-preview-value" role="status">
        <code v-if="proxyAddress">{{ proxyAddress }}</code>
        <span v-else class="preview-placeholder">{{ t("settings.proxy.previewPlaceholder") }}</span>
      </div>
    </div>

    <p v-if="!desktop" class="proxy-notice">{{ t("settings.proxy.desktopOnly") }}</p>
    <p v-if="testing" class="proxy-notice" role="status">
      {{ t(saving ? "settings.proxy.saving" : "settings.proxy.testing") }}</p>
    <ElAlert v-else-if="testResult" :closable="false" :description="t('settings.proxy.testSuccessDetails', {target: testResult.target, elapsed: testResult.elapsedMs})" :title="t(!dirty && !saveError ? 'settings.proxy.testSuccessSaved' : 'settings.proxy.testSuccess')" class="proxy-alert" role="status"
             show-icon
             type="success"/>
    <template v-for="({error, title}, index) in failures" :key="index">
      <ElAlert :closable="false" :description="errorMessage(error.code)" :title="t(title)"
               class="proxy-alert" show-icon type="error"/>
      <details v-if="error.detail" class="proxy-error-details">
        <summary>{{ t("settings.requiredTools.errorDetails") }}</summary>
        <p>{{ error.detail }}</p>
      </details>
    </template>

    <footer class="proxy-actions">
      <span v-if="desktop && ready" class="proxy-save-status" role="status">
        {{
          t(saving ? 'settings.proxy.saving' : dirty ? 'settings.proxy.unsaved' : saved ? 'settings.proxy.saved' : 'settings.proxy.disabled')
        }}
      </span>
      <ElButton v-if="loadError" :loading="loading" @click="load">{{ t("settings.proxy.retryLoad") }}</ElButton>
      <ElButton :disabled="!canTest" :loading="testing" @click="testConnection">{{
          t("settings.proxy.testConnection")
        }}
      </ElButton>
      <ElButton :disabled="!canSave" :loading="saving" type="primary" @click="save">{{
          t("settings.proxy.save")
        }}
      </ElButton>
    </footer>
  </section>
</template>

<style scoped>
.proxy-settings-card {
  min-width: 0;
  padding: 22px;
  border: 1px solid var(--app-border);
  border-radius: var(--app-radius);
  background: var(--app-surface);
}

.proxy-fields {
  display: grid;
  grid-template-columns: 150px minmax(0, 1fr) 130px;
  gap: 16px;
}

.proxy-field {
  min-width: 0;
}

.proxy-field label {
  display: block;
  margin-bottom: 9px;
  color: var(--app-text);
  font-size: 13px;
  font-weight: 500;
}

.proxy-field :deep(.el-select) {
  width: 100%;
}

.proxy-field.is-invalid :deep(.el-input__wrapper) {
  box-shadow: 0 0 0 1px var(--app-danger) inset;
}

.proxy-field.is-invalid .field-hint {
  color: var(--app-danger);
}

.field-hint {
  margin: 8px 0 0;
  color: var(--app-text-secondary);
  font-size: 11px;
  line-height: 1.7;
}

.proxy-preview {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 8px 20px;
  margin-top: 22px;
  padding: 14px 16px;
  border: 1px solid var(--app-border);
  border-radius: 8px;
  background: var(--app-background);
  font-size: 12px;
}

.proxy-preview > span {
  flex-shrink: 0;
  color: var(--app-text-secondary);
}

.proxy-preview-value {
  min-width: 0;
  overflow-wrap: anywhere;
}

.proxy-preview code {
  color: var(--app-accent);
  font-family: Consolas, "SFMono-Regular", monospace;
  font-size: 12px;
}

.preview-placeholder {
  color: var(--app-text-muted);
}

.proxy-notice {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  margin: 18px 0 0;
  color: var(--app-text-secondary);
  font-size: 12px;
  line-height: 1.7;
}

.proxy-alert {
  min-height: 64px;
  margin-top: 14px;
  border-radius: 8px;
}

.proxy-alert :deep(.el-alert__content) {
  min-width: 0;
}

.proxy-alert :deep(.el-alert__title),
.proxy-alert :deep(.el-alert__description) {
  font-size: 12px;
  line-height: 1.7;
  overflow-wrap: anywhere;
}

.proxy-error-details {
  margin: 8px 16px 0 56px;
  color: var(--app-text-secondary);
  font-size: 12px;
  line-height: 1.7;
  overflow-wrap: anywhere;
}

.proxy-error-details p {
  margin: 10px 0 0;
}

.proxy-error-details summary {
  cursor: pointer;
}

.proxy-save-status {
  margin-right: auto;
  color: var(--app-text-secondary);
  font-size: 12px;
}

.proxy-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 20px;
  padding-top: 18px;
  border-top: 1px solid var(--app-border);
}

.proxy-actions :deep(.el-button) {
  margin-left: 0;
  font-size: 12px;
}

@media (max-width: 800px) {
  .proxy-fields {
    grid-template-columns: minmax(0, 1fr);
    gap: 20px;
  }
}
</style>
