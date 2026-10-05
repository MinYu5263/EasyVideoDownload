<script lang="ts" setup>
import {ref, watch} from "vue";
import {ElButton, ElInput, ElOption, ElSelect} from "element-plus";
import {useI18n} from "vue-i18n";
import {useProxySettings} from "../composables/useProxySettings";
import {useFeedback} from "../composables/useFeedback";

const {t} = useI18n({useScope: "global"});
const protocols = [
  {value: "http", label: "HTTP"},
  {value: "https", label: "HTTPS"},
  {value: "socks5", label: "SOCKS5"},
] as const;
const {
  desktop, draft, ready, loading, saving, testing, addressInvalid, canSave, canTest,
  proxyAddress, loadError, saveError, testError, testResult, save, testConnection, load
} = useProxySettings();
const addressErrorVisible = ref(false);
watch(() => draft.address, () => {
  addressErrorVisible.value = false;
}, {flush: "sync"});
watch(addressInvalid, invalid => {
  if (!invalid) addressErrorVisible.value = false;
}, {flush: "sync"});

function validateAddress() {
  addressErrorVisible.value = addressInvalid.value;
}

function updatePort(value: string) {
  if (value === "" || (/^\d{1,5}$/.test(value) && Number(value) >= 1 && Number(value) <= 65535)) draft.port = value;
}

const {watchError, inform} = useFeedback();
watchError(loadError, error => errorMessage(error.code), () => ({
  key: 'proxy:load',
  title: t('settings.proxy.loadFailedTitle')
}));
watchError(saveError, error => errorMessage(error.code), () => ({
  key: 'proxy:save',
  title: t('settings.proxy.saveFailedTitle')
}));
watchError(testError, error => errorMessage(error.code), () => ({
  key: 'proxy:test',
  title: t('settings.proxy.testFailedTitle')
}));
watch(testing, (value, previous) => {
  if (!value && previous && testResult.value) inform(t('settings.proxy.testSuccessDetails', {
    target: testResult.value.target, elapsed: testResult.value.elapsedMs
  }));
});

async function saveProxy() {
  if (await save()) inform(t('settings.proxy.saved'));
}

function errorMessage(code: string) {
  const known = ["loadFailed", "saveFailed", "invalidSettings", "bridgeFailed", "timeout", "connectionRefused",
    "proxyAuthRequired", "tlsFailed", "proxyConnectionFailed", "requestFailed", "targetHttpError", "responseTooLarge", "testUnavailable"];
  return t(`settings.proxy.errors.${known.includes(code) ? code : "bridgeFailed"}`);
}
</script>

<template>
  <section v-loading="testing" :aria-busy="loading || saving || testing" :aria-label="t('settings.proxy.title')"
           :inert="testing"
           class="proxy-settings-card">
    <div class="proxy-fields">
      <div class="proxy-field">
        <label for="proxy-protocol">{{ t("settings.proxy.protocol") }}</label>
        <ElSelect id="proxy-protocol" v-model="draft.protocol" :aria-label="t('settings.proxy.protocol')"
                  :aria-describedby="draft.protocol === 'socks5' ? 'proxy-protocol-hint' : undefined"
                  :disabled="!ready || saving || testing">
          <ElOption v-for="protocol in protocols" :key="protocol.value" :label="protocol.label"
                    :value="protocol.value"/>
        </ElSelect>
        <p v-if="draft.protocol === 'socks5'" id="proxy-protocol-hint" class="field-hint">
          {{ t("settings.proxy.socksHint") }}
        </p>
      </div>

      <div :class="{'is-invalid': addressErrorVisible}" class="proxy-field">
        <label for="proxy-address">{{ t("settings.proxy.address") }}</label>
        <ElInput id="proxy-address" v-model="draft.address" :aria-describedby="addressErrorVisible ? 'proxy-address-hint' : undefined"
                 :aria-invalid="addressErrorVisible"
                 :aria-label="t('settings.proxy.address')"
                 :disabled="!ready || saving || testing"
                 :placeholder="t('settings.proxy.addressPlaceholder')"
                 autocomplete="off" clearable spellcheck="false" @blur="validateAddress"/>
      </div>

      <div class="proxy-field">
        <label for="proxy-port">{{ t("settings.proxy.port") }}</label>
        <ElInput id="proxy-port" :aria-label="t('settings.proxy.port')" :disabled="!ready || saving || testing"
                 :model-value="draft.port"
                 :placeholder="t('settings.proxy.portPlaceholder')"
                 autocomplete="off" inputmode="numeric" @update:model-value="updatePort"/>
      </div>
    </div>

    <div :class="{'is-invalid': addressErrorVisible}" class="proxy-preview">
      <span id="proxy-preview-label">{{ t("settings.proxy.addressPreview") }}</span>
      <div :title="addressErrorVisible ? t('settings.proxy.addressInvalid') : proxyAddress || undefined"
           aria-labelledby="proxy-preview-label" aria-live="polite" class="proxy-preview-value" role="status">
        <span v-if="addressErrorVisible" id="proxy-address-hint">{{ t("settings.proxy.addressInvalid") }}</span>
        <code v-else-if="proxyAddress">{{ proxyAddress }}</code>
        <span v-else class="preview-placeholder">{{ t("settings.proxy.previewPlaceholder") }}</span>
      </div>
    </div>

    <p v-if="!desktop" class="proxy-notice">{{ t("settings.proxy.desktopOnly") }}</p>

    <footer class="proxy-actions">
      <ElButton v-if="loadError" :loading="loading" @click="load">{{ t("settings.proxy.retryLoad") }}</ElButton>
      <ElButton :disabled="!canTest" :loading="testing" @click="testConnection">{{
          t("settings.proxy.testConnection")
        }}
      </ElButton>
      <ElButton :disabled="!canSave" :loading="saving" type="primary" @click="saveProxy">{{
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

.proxy-settings-card :deep(.el-loading-mask) {
  border-radius: inherit;
  background: var(--el-mask-color, rgba(255, 255, 255, .8));
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

.field-hint {
  margin: 8px 0 0;
  color: var(--app-text-secondary);
  font-size: 11px;
  line-height: 1.7;
}

.proxy-preview {
  display: flex;
  align-items: center;
  gap: 8px 20px;
  margin-top: 22px;
  padding: 14px 16px;
  border: 1px solid var(--app-border);
  border-radius: 8px;
  background: var(--app-background);
  font-size: 12px;
  line-height: 18px;
}

.proxy-preview.is-invalid {
  border-color: var(--app-danger);
  background: var(--app-danger-soft);
}

.proxy-preview.is-invalid > span,
.proxy-preview.is-invalid .proxy-preview-value {
  color: var(--app-danger);
}

.proxy-preview > span {
  flex-shrink: 0;
  color: var(--app-text-secondary);
}

.proxy-preview-value {
  flex: 1;
  min-width: 0;
  height: 18px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
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







.proxy-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 20px;
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
