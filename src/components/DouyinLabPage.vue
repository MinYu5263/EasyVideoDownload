<script lang="ts" setup>
import {computed, onMounted, onUnmounted, ref, watch} from "vue";
import {useI18n} from "vue-i18n";
import {
  ElAlert,
  ElButton,
  ElInput,
  ElProgress,
  ElRadio,
  ElScrollbar,
  ElTable,
  ElTableColumn,
  ElTag
} from "element-plus";
import {useDouyinLab} from "../composables/useDouyinLab";
import {useDesktopActions} from "../composables/useDesktopActions";
import type {LabError} from "../composables/douyinLabTypes";

const props = defineProps<{ active: boolean }>();
const {t, te} = useI18n({useScope: "global"});
const lab = useDouyinLab(), desktop = useDesktopActions();
const {state, ready, connecting, error, connectionError, selectedFormatId, directory, active, busy} = lab;
const link = ref(""), nativeError = ref<LabError | null>(null), choosing = ref(false);
onMounted(lab.connect);
onUnmounted(lab.dispose);
watch(() => props.active, value => {
  if (value) void lab.refresh();
});
const problem = computed(() => nativeError.value ?? connectionError.value ?? error.value ?? state.value?.error);
const message = computed(() => {
  const e = problem.value;
  if (!e) return "";
  return te(`douyinLab.errors.${e.code}`) ? t(`douyinLab.errors.${e.code}`) : e.detail || t("douyinLab.errors.bridgeFailed");
});
const canParse = computed(() => ready.value && !busy.value && Boolean(link.value.trim()) && state.value?.cookieConfigured);
const canDownload = computed(() => ready.value && !busy.value && Boolean(lab.selectedFormat.value) && Boolean(directory.value) && state.value?.ffprobeAvailable);
const percentage = computed(() => state.value?.totalBytes ? Math.min(100, Math.floor(state.value.receivedBytes / state.value.totalBytes * 100)) : null);
const size = (value: number | null | undefined) => value == null ? t("douyinLab.unknown") : value >= 1024 * 1024 ? `${(value / 1024 / 1024).toFixed(1)} MB` : `${(value / 1024).toFixed(1)} KB`;
const bitrate = (value: number | null | undefined) => value == null ? t("douyinLab.unknown") : `${Math.round(value / 1000)} kbps`;
const dimensions = (format: Record<string, unknown>) => typeof format.width === 'number' && typeof format.height === 'number' ? `${format.width} × ${format.height}` : t("douyinLab.unknown");

function failed(e: unknown) {
  nativeError.value = typeof e === "object" && e !== null && "code" in e ? e as LabError : {
    code: "nativeOperationFailed",
    detail: String(e)
  };
}

async function paste() {
  nativeError.value = null;
  try {
    link.value = await desktop.readClipboard();
  } catch (e) {
    failed(e);
  }
}

async function chooseDirectory() {
  choosing.value = true;
  nativeError.value = null;
  try {
    const chosen = await desktop.chooseDirectory();
    if (chosen) directory.value = chosen;
  } catch (e) {
    failed(e);
  } finally {
    choosing.value = false;
  }
}

async function parse() {
  nativeError.value = null;
  await lab.parse(link.value.trim());
}
</script>

<template>
  <ElScrollbar class="lab-scrollbar">
    <section aria-labelledby="page-title" class="lab-content">
      <ElAlert :closable="false" :title="t('douyinLab.notice')" show-icon type="info"/>
      <div class="lab-status">
        <ElTag :type="state?.cookieConfigured ? 'success' : 'warning'">
          {{ t(state?.cookieConfigured ? 'douyinLab.cookieReady' : 'douyinLab.cookieMissing') }}
        </ElTag>
        <ElTag :type="state?.ffprobeAvailable ? 'success' : 'warning'">
          {{ t(state?.ffprobeAvailable ? 'douyinLab.probeReady' : 'douyinLab.probeMissing') }}
        </ElTag>
        <ElButton :disabled="connecting" :loading="connecting" @click="lab.connect">{{ t('douyinLab.refresh') }}
        </ElButton>
      </div>
      <ElAlert v-if="problem" :closable="false" :title="message" role="alert" show-icon type="error"/>
      <form class="lab-input" @submit.prevent="parse">
        <label for="douyin-lab-link">{{ t('douyinLab.link') }}</label>
        <div class="lab-row">
          <ElInput id="douyin-lab-link" v-model="link" :disabled="busy" :placeholder="t('douyinLab.linkPlaceholder')"
                   clearable/>
          <ElButton :disabled="!ready || busy" @click="paste">{{ t('douyinLab.paste') }}</ElButton>
          <ElButton :disabled="!canParse" :loading="state?.phase === 'parsing'" native-type="submit" type="primary">
            {{ t('douyinLab.parse') }}
          </ElButton>
        </div>
      </form>
      <template v-if="state?.parsed">
        <div class="lab-video">
          <img v-if="state.parsed.cover" :alt="t('douyinLab.cover')" :src="state.parsed.cover"
               referrerpolicy="no-referrer"/>
          <div><h2>{{ state.parsed.title }}</h2>
            <p>{{ t('douyinLab.videoId') }} {{ state.parsed.videoId }}</p></div>
        </div>
        <ElTable :aria-label="t('douyinLab.formats')" :data="state.parsed.formats" class="lab-formats" row-key="id">
          <ElTableColumn :label="t('douyinLab.select')" width="90">
            <template #default="{row}">
              <ElRadio v-model="selectedFormatId" :aria-label="`${dimensions(row)} ${row.codec} ${bitrate(row.bitrate)}`" :disabled="busy"
                       :value="row.id"><span
                  class="sr-only">{{ t('douyinLab.select') }}</span></ElRadio>
            </template>
          </ElTableColumn>
          <ElTableColumn :label="t('douyinLab.resolution')" min-width="145">
            <template #default="{row}">{{ dimensions(row) }}</template>
          </ElTableColumn>
          <ElTableColumn :label="t('douyinLab.codec')" width="100">
            <template #default="{row}">
              {{ row.codec === 'unknown' ? t('douyinLab.unknown') : row.codec === 'hevc' ? 'H.265' : 'H.264' }}
            </template>
          </ElTableColumn>
          <ElTableColumn :label="t('douyinLab.fps')" width="90">
            <template #default="{row}">{{ row.fps ?? t('douyinLab.unknown') }}</template>
          </ElTableColumn>
          <ElTableColumn :label="t('douyinLab.bitrate')" min-width="115">
            <template #default="{row}">{{ bitrate(row.bitrate) }}</template>
          </ElTableColumn>
          <ElTableColumn :label="t('douyinLab.fileSize')" width="105">
            <template #default="{row}">{{ size(row.fileSize) }}</template>
          </ElTableColumn>
          <ElTableColumn :label="t('douyinLab.watermark')" width="100">
            <template #default="{row}">
              {{ t(row.watermarked === null ? 'douyinLab.unknown' : row.watermarked ? 'douyinLab.yes' : 'douyinLab.no') }}
            </template>
          </ElTableColumn>
        </ElTable>
      </template>
      <div class="lab-output">
        <label for="douyin-lab-directory">{{ t('douyinLab.directory') }}</label>
        <div class="lab-row">
          <ElInput id="douyin-lab-directory" :model-value="directory" readonly/>
          <ElButton :disabled="!ready || busy || choosing" :loading="choosing" @click="chooseDirectory">
            {{ t('douyinLab.chooseDirectory') }}
          </ElButton>
          <ElButton :disabled="!canDownload || choosing" type="primary" @click="lab.download">
            {{ t('douyinLab.download') }}
          </ElButton>
        </div>
      </div>
      <div v-if="state && state.phase !== 'idle'" aria-live="polite" class="lab-progress">
        <div class="lab-row"><strong>{{ t(`douyinLab.phase.${state.phase}`) }}</strong>
          <ElButton v-if="active" :disabled="state.phase === 'cancelling' || lab.pending.value" @click="lab.cancel">
            {{ t('douyinLab.cancel') }}
          </ElButton>
        </div>
        <template v-if="['downloading','verifying','cancelling','completed'].includes(state.phase)">
          <ElProgress v-if="percentage !== null" :percentage="percentage"
                      :status="state.phase === 'completed' ? 'success' : undefined"/>
          <p>{{ size(state.receivedBytes) }}<span
              v-if="state.totalBytes !== null"> / {{ size(state.totalBytes) }}</span></p>
        </template>
      </div>
      <div v-if="state?.observed && state.outputPath" class="lab-result">
        <h2>{{ t('douyinLab.verified') }}</h2>
        <p>{{ state.observed.width }} × {{ state.observed.height }} · {{ state.observed.codec }} ·
          {{ bitrate(state.observed.bitrate) }} · {{ size(state.observed.fileSize) }}</p>
        <p v-if="state.observed.duration">
          {{ t('douyinLab.duration', {seconds: state.observed.duration.toFixed(2)}) }}</p>
        <p class="lab-path">{{ state.outputPath }}</p>
      </div>
    </section>
  </ElScrollbar>
</template>

<style scoped>
.lab-scrollbar {
  flex: 1;
  min-height: 0;
}

.lab-content {
  display: flex;
  flex-direction: column;
  gap: 20px;
  padding: 0 var(--app-page-padding-x) var(--app-page-padding-bottom);
}

.lab-status, .lab-row {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}

.lab-row :deep(.el-input) {
  flex: 1;
  min-width: 250px;
}

.lab-input, .lab-output {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.lab-video {
  display: flex;
  gap: 16px;
  align-items: center;
}

.lab-video img {
  width: 100px;
  height: 100px;
  object-fit: cover;
  border-radius: 8px;
}

h2 {
  font-size: 16px;
  line-height: 1.5;
  margin: 0 0 8px;
  color: var(--app-text);
}

p {
  font-size: 13px;
  color: var(--app-text-secondary);
  margin: 8px 0;
}

.lab-result {
  border: 1px solid var(--app-border);
  border-radius: var(--app-radius);
  padding: 16px;
}

.lab-path {
  overflow-wrap: anywhere;
  user-select: text;
}

.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}
</style>
