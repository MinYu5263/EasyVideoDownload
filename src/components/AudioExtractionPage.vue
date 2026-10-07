<script lang="ts" setup>
import {computed, onMounted, onUnmounted, ref, watch} from 'vue';
import {ElButton, ElIcon, ElOption, ElProgress, ElScrollbar, ElSelect, ElUpload} from 'element-plus';
import {Close, Document, Loading, UploadFilled} from '@element-plus/icons-vue';
import {invoke} from '@tauri-apps/api/core';
import {getCurrentWebview} from '@tauri-apps/api/webview';
import type {UnlistenFn} from '@tauri-apps/api/event';
import {useI18n} from 'vue-i18n';
import {useAudioExtraction} from '../composables/useAudioExtraction';
import {useFeedback} from '../composables/useFeedback';
import {persistenceError} from '../composables/useUiPreferences';

const props = defineProps<{ active: boolean }>();
const {t, locale} = useI18n({useScope: 'global'});
const {inform, notifyError} = useFeedback();
const audio = useAudioExtraction();
const {state, selectedTrack, track, busy, ready} = audio;
const uploadArea = ref<HTMLElement>();
const choosing = ref(false), dragging = ref(false);
const disabled = computed(() => busy.value || choosing.value || !ready.value);
let dropListener: UnlistenFn | undefined, disposed = false;
const unknown = computed(() => t('audio.unknown'));
const number = (value: number) => new Intl.NumberFormat(locale.value, {maximumFractionDigits: 1}).format(value);
const duration = computed(() => {
  if (!track.value?.duration) return unknown.value;
  const seconds = Math.floor(track.value.duration);
  return [Math.floor(seconds / 3600), Math.floor(seconds / 60) % 60, seconds % 60]
      .filter((_v, i) => i !== 0 || seconds >= 3600).map(v => String(v).padStart(2, '0')).join(':');
});
const details = computed(() => [
  {label: t('audio.duration'), value: duration.value},
  {label: t('audio.codec'), value: track.value?.codecLabel || unknown.value},
  {label: t('audio.outputFormat'), value: track.value?.outputFormat || unknown.value},
  {
    label: t('audio.bitrate'),
    value: track.value?.bitrate ? `${number(track.value.bitrate / 1000)} kbps` : unknown.value
  },
  {
    label: t('audio.sampleRate'),
    value: track.value?.sampleRate ? `${number(track.value.sampleRate / 1000)} kHz` : unknown.value
  },
  {
    label: t('audio.channels'),
    value: track.value?.channels ? t('audio.channelCount', {count: track.value.channels}) : unknown.value
  },
]);

function failure(cause: unknown) {
  notifyError(t('common.error'), {detail: persistenceError(cause).detail, key: 'audio:operation'});
}

async function parse(path: string) {
  try {
    if (await audio.parse(path)) inform(t('audio.parseSuccess'));
  } catch (error) {
    failure(error);
  }
}

async function choose() {
  if (disabled.value) return;
  choosing.value = true;
  try {
    const path = await invoke<string | null>('select_audio_source_file');
    if (path) await parse(path);
  } catch (error) {
    failure(error);
  } finally {
    choosing.value = false;
  }
}

async function extract() {
  try {
    if (await audio.extract()) inform(t('audio.extractSuccess'));
  } catch (error) {
    failure(error);
  }
}

async function clear() {
  try {
    await audio.clear();
  } catch (error) {
    failure(error);
  }
}

watch(() => props.active, active => {
  if (!active) dragging.value = false;
});
onMounted(async () => {
  if (!audio.desktop) return;
  try {
    await audio.connect();
    if (disposed) return;
    const stop = await getCurrentWebview().onDragDropEvent(({payload}) => {
      if (disposed || !props.active || disabled.value) {
        dragging.value = false;
        return;
      }
      if (payload.type === 'leave') {
        dragging.value = false;
        return;
      }
      const rect = uploadArea.value?.getBoundingClientRect();
      const x = payload.position.x / window.devicePixelRatio, y = payload.position.y / window.devicePixelRatio;
      const inside = !!rect && x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
      dragging.value = payload.type !== 'drop' && inside;
      if (payload.type === 'drop' && inside) {
        if (payload.paths.length !== 1) inform(t('audio.singleFile'), 'warning');
        else void parse(payload.paths[0]!);
      }
    });
    if (disposed) stop(); else dropListener = stop;
  } catch (error) {
    failure(error);
  }
});
onUnmounted(() => {
  disposed = true;
  dropListener?.();
  audio.dispose();
});
</script>

<template>
  <section :aria-label="t('navigation.audio')" class="audio-page">
    <ElScrollbar height="100%" view-class="audio-scroll-content">
      <div class="audio-content">
        <div ref="uploadArea" :aria-disabled="disabled" :aria-label="t('audio.chooseFile')"
             :class="{dragging, busy: disabled}" :tabindex="disabled ? -1 : 0" class="audio-upload" role="button"
             @click.capture.stop.prevent="choose" @keydown.enter.capture.stop.prevent="choose"
             @keydown.space.capture.stop.prevent="choose">
          <!-- ElUpload is visual only. Tauri supplies native paths; its HTML input stays disabled. -->
          <ElUpload :auto-upload="false" :multiple="false" :show-file-list="false" aria-hidden="true" disabled drag
                    inert>
            <ElIcon class="el-icon--upload">
              <UploadFilled/>
            </ElIcon>
            <div class="el-upload__text">{{ t('audio.dropFile') }} <em>{{ t('audio.clickToChoose') }}</em></div>
          </ElUpload>
        </div>
        <p v-if="!audio.desktop" class="audio-note">{{ t('audio.desktopRequired') }}</p>
        <div v-if="state.phase === 'parsing'" class="audio-parsing" role="status">
          <ElIcon class="is-loading">
            <Loading/>
          </ElIcon>
          {{ t('audio.parsing') }}
        </div>
        <section v-if="state.info" :aria-label="t('audio.audioInfo')" class="audio-card">
          <div class="audio-file">
            <ElIcon class="audio-file-icon">
              <Document/>
            </ElIcon>
            <span :title="state.info.fileName" class="audio-file-name">{{ state.info.fileName }}</span>
            <ElButton :aria-label="t('audio.removeFile')" :disabled="disabled" circle text @click="clear">
              <ElIcon>
                <Close/>
              </ElIcon>
            </ElButton>
          </div>
          <div v-if="state.info.tracks.length > 1" class="audio-track">
            <label for="audio-track-select">{{ t('audio.track') }}</label>
            <ElSelect id="audio-track-select" v-model="selectedTrack" :aria-label="t('audio.track')"
                      :disabled="disabled">
              <ElOption v-for="(item, index) in state.info.tracks" :key="item.index" :label="[t('audio.trackNumber', {number: index + 1}), item.title, item.codecLabel, item.isDefault ? t('audio.defaultTrack') : ''].filter(Boolean).join(' · ')"
                        :value="item.index"/>
            </ElSelect>
          </div>
          <dl class="audio-details">
            <div v-for="detail in details" :key="detail.label">
              <dt>{{ detail.label }}</dt>
              <dd>{{ detail.value }}</dd>
            </div>
          </dl>
          <div v-if="state.phase === 'extracting'" class="audio-progress" role="status">
            <ElProgress :indeterminate="state.progress === null" :percentage="state.progress ?? 0"
                        :show-text="state.progress !== null" :stroke-width="5"/>
          </div>
          <div class="audio-actions">
            <ElButton :disabled="disabled || !track" :loading="state.phase === 'extracting'" size="large" type="primary"
                      @click="extract">
              {{ t(state.phase === 'extracting' ? 'audio.extracting' : 'audio.extract') }}
            </ElButton>
          </div>
        </section>
      </div>
    </ElScrollbar>
  </section>
</template>

<style scoped>
.audio-page {
  height: 100%;
  min-height: 0;
  width: 100%;
}

.audio-content {
  padding: 10px var(--app-page-padding-x) var(--app-page-padding-bottom);
  max-width: 1060px;
}

.audio-upload {
  border-radius: 12px;
  cursor: pointer;
}

.audio-upload:focus-visible {
  outline: 2px solid var(--app-accent);
  outline-offset: 4px;
}

.audio-upload.busy {
  cursor: default;
}

.audio-upload :deep(.el-upload) {
  width: 100%;
  pointer-events: none;
}

.audio-upload :deep(.el-upload.is-disabled) {
  --el-disabled-bg-color: var(--app-surface);
  --el-text-color-placeholder: var(--app-text-secondary);
  --el-disabled-text-color: var(--app-accent);
}

.audio-upload :deep(.el-upload-dragger) {
  padding: 36px 20px 32px;
  border-radius: 12px;
  background: var(--app-surface);
  cursor: inherit;
  border-color: var(--app-border);
}

.audio-upload:not(.busy):hover :deep(.el-upload-dragger), .audio-upload.dragging :deep(.el-upload-dragger) {
  border-color: var(--app-accent);
  background: var(--app-accent-soft);
}

.audio-upload :deep(.el-icon--upload) {
  color: var(--app-text-muted);
  font-size: 48px;
  margin-bottom: 14px;
  line-height: 1;
}

.audio-upload :deep(.el-upload__text) {
  color: var(--app-text-secondary);
}

.audio-upload :deep(em) {
  color: var(--app-accent);
}

.audio-note, .audio-parsing {
  color: var(--app-text-secondary);
  margin: 20px 0;
}

.audio-parsing {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 28px;
}

.audio-card {
  margin-top: 24px;
  padding: 20px 24px 24px;
  background: var(--app-surface);
  border: 1px solid var(--app-border);
  border-radius: 12px;
}

.audio-file {
  display: flex;
  gap: 10px;
  align-items: center;
  padding-bottom: 18px;
  border-bottom: 1px solid var(--app-border);
}

.audio-file-icon {
  font-size: 21px;
  color: var(--app-accent);
}

.audio-file-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  font-weight: 500;
}

.audio-track {
  display: flex;
  align-items: center;
  gap: 16px;
  margin-top: 20px;
}

.audio-track label {
  color: var(--app-text-secondary);
  flex-shrink: 0;
}

.audio-track :deep(.el-select) {
  flex: 1;
  min-width: 0;
  max-width: 420px;
}

.audio-details {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 22px 20px;
  margin: 24px 0;
}

.audio-details dt {
  color: var(--app-text-muted);
  font-size: 12px;
  margin-bottom: 5px;
}

.audio-details dd {
  margin: 0;
  color: var(--app-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.audio-progress {
  margin: 0 0 20px;
}

.audio-actions {
  display: flex;
  justify-content: flex-end;
}

.audio-actions :deep(.el-button) {
  min-width: 124px;
}

@media (max-width: 1000px) {
  .audio-details {
    gap: 20px 16px;
  }
}
</style>
