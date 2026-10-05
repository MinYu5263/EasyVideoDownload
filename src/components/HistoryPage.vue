<script lang="ts" setup>
import {computed, h, nextTick, ref, watch} from "vue";
import {
  ElButton,
  ElEmpty,
  ElInput,
  ElMessage,
  ElMessageBox,
  ElScrollbar,
  ElTooltip,
  type ScrollbarInstance
} from "element-plus";
import {ArrowLeft, RefreshRight, Search} from "@element-plus/icons-vue";
import {useI18n} from "vue-i18n";
import {type DownloadRecord, type HistoryStatus, useDownloadHistory} from "../composables/useDownloadHistory";
import {useDownloadHistoryActions} from "../composables/downloadHistoryActions";
import {groupHistoryRecords, sanitizeHistoryDetail} from "../composables/downloadHistoryDisplay";
import {useDownloadTasks} from "../composables/useDownloadTasks";
import {createHistoryRedownload} from "../composables/useHistoryRedownload";
import {createHistoryFileRecovery} from "../composables/useHistoryFileRecovery";
import DownloadHistoryCard from "./DownloadHistoryCard.vue";
import DownloadHistoryDetails from "./DownloadHistoryDetails.vue";

const props = withDefaults(defineProps<{ active: boolean; downloading?: boolean }>(), {downloading: false});
const {t} = useI18n({useScope: "global"});
const history = useDownloadHistory(), actions = useDownloadHistoryActions(), tasks = useDownloadTasks();
const redownload = createHistoryRedownload({tasks});
const recovery = createHistoryFileRecovery({
  actions,
  confirmRedownload: async () => confirm(t("tasks.missingMessage"), t("tasks.missingTitle"), t("history.prepare")),
  redownload: record => restartRecord(record)
});
const scrollbar = ref<ScrollbarInstance>(), search = ref(history.filters.value.query),
    selected = ref<DownloadRecord | null>(null), drawer = ref(false);
const retainedRetries = ref<Record<number, { record: DownloadRecord; previousRequestIds: string[] }>>({});
let retryContext = 0;
watch(() => [history.filters.value.status, history.filters.value.query, history.trashed.value, props.active], () => {
  retryContext++;
  retainedRetries.value = {};
}, {flush: "sync"});
watch(history.records, rows => {
  const retained = {...retainedRetries.value};
  let changed = false;
  for (const record of rows) if (retained[record.id] && !retained[record.id].previousRequestIds.includes(record.requestId)) {
    delete retained[record.id];
    changed = true;
  }
  if (changed) retainedRetries.value = retained;
});

async function restartRecord(record: DownloadRecord) {
  if (record.deletedAt || ['queued', 'running'].includes(record.status)) return;
  const context = retryContext, previous = retainedRetries.value[record.id];
  // History may still show an earlier attempt when a fast failure is retried again.
  const previousRequestIds = [...new Set([record.requestId, ...(previous?.record.requestId === record.requestId ? previous.previousRequestIds : [])])];
  drawer.value = false;
  actions.clearFeedback();
  retainedRetries.value = {...retainedRetries.value, [record.id]: {record, previousRequestIds}};
  if (await redownload.run(record) && context === retryContext) {
    const current = history.records.value.find(row => row.id === record.id);
    if (!current || previousRequestIds.includes(current.requestId)) retainedRetries.value = {
      ...retainedRetries.value,
      [record.id]: {record: tasks.taskFor(record.id)?.record ?? record, previousRequestIds}
    };
  }
}

const statuses: (HistoryStatus | null)[] = [null, "running", "completed", "failed", "cancelled", "interrupted"];
let returnFocus: HTMLElement | null = null;
watch(() => props.active, active => {
  if (active) void history.refresh(); else drawer.value = false;
}, {immediate: true});
watch(history.records, rows => {
  if (selected.value) {
    const updated = rows.find(r => r.id === selected.value!.id);
    if (updated) selected.value = updated; else if (!retainedRetries.value[selected.value.id]) {
      drawer.value = false;
      selected.value = null;
    }
  }
});
const filtered = computed(() => Boolean(history.filters.value.query || history.filters.value.status));
const groups = computed(() => {
  const rows = new Map(history.records.value.map(record => [record.id, record]));
  if (!history.trashed.value) for (const {
    record,
    previousRequestIds
  } of Object.values(retainedRetries.value)) if (!rows.has(record.id) || previousRequestIds.includes(rows.get(record.id)!.requestId)) rows.set(record.id, record);
  return groupHistoryRecords([...rows.values()].map(tasks.mergeRecord).sort((a, b) => b.startedAt.localeCompare(a.startedAt) || b.id - a.id));
});
const retainedOutsideFilter = computed(() => !history.trashed.value && Object.values(retainedRetries.value).some(({record}) => !history.records.value.some(row => row.id === record.id)));

function taskForRecord(record: DownloadRecord | null) {
  const task = record ? tasks.taskFor(record.id) : null;
  return record && task?.record.requestId === record.requestId ? task : null;
}

const displayedSelected = computed(() => selected.value ? tasks.mergeRecord(selected.value) : null);

async function revealRecord(id: number) {
  search.value = "";
  await history.setTrashed(false);
  await history.setFilters({status: null, query: ""});
  while (!history.records.value.some(row => row.id === id) && history.nextCursor.value) {
    if (!await history.loadMore()) break;
  }
  const record = history.records.value.find(row => row.id === id);
  if (record) {
    await nextTick();
    await action("details", tasks.mergeRecord(record));
  }
}

defineExpose({revealRecord});
const feedbackError = computed(() => {
  const error = actions.error.value;
  if (!error) return null;
  const key = `history.${error.code}`;
  const known = ["desktopOnly", "recordNotFound", "recordRunning", "recordTrashed", "recordNotTrashed", "historyBusy", "historyFileUnavailable", "historyFileUnsafe", "historyFileChanged", "historyRecycleFailed", "historyFileRecycledSaveFailed", "historyFileMissing", "historyDirectoryMissing", "historyOpenFailed", "loadFailed", "saveFailed"];
  const deletionErrors = ["historyFilePermissionDenied", "historyFileReadOnly", "historyFileOccupied", "historyFileDeleteFailed", "historyFileInUse", "historyFileDeletedSaveFailed", "historyTrashPartiallyDeleted", "historyFileDeletionUnsupported"];
  return `${t(known.includes(error.code) || deletionErrors.includes(error.code) ? key : "history.operationFailed")}${error.detail ? ` ${sanitizeHistoryDetail(error.detail)}` : ""}`;
});
const feedbackSuccess = computed(() => actions.success.value ? t(`history.${actions.success.value}`) : null);

async function filter(status: HistoryStatus | null) {
  scrollbar.value?.setScrollTop(0);
  await history.setFilters({status, query: search.value});
}

function searchChanged(value: string) {
  search.value = value;
  scrollbar.value?.setScrollTop(0);
  void history.setFilters({query: value});
}

async function restoreFocus() {
  await nextTick();
  if (!props.active) return;
  if (returnFocus?.isConnected) returnFocus.focus(); else document.querySelector<HTMLElement>("#main-content")?.focus();
}

function toast(message: string, type: "success" | "error" | "info" = "success") {
  actions.clearFeedback();
  ElMessage({showClose: false, duration: 2000, message, type, grouping: true});
}

function failed() {
  toast(feedbackError.value ?? t("history.operationFailed"), "error");
}

async function changed(record?: DownloadRecord) {
  if (record) {
    const rows = {...retainedRetries.value};
    delete rows[record.id];
    retainedRetries.value = rows;
  } else retainedRetries.value = {};
  if (!record || selected.value?.id === record.id) drawer.value = false;
  await history.refresh();
}

async function restore(record: DownloadRecord) {
  if (actions.busy.value) return;
  if (await actions.restore(record)) {
    toast(t("history.restored"));
    await changed(record);
  } else failed();
}

async function confirm(message: string | ReturnType<typeof h>, title: string, button: string) {
  try {
    await ElMessageBox.confirm(message, title, {
      confirmButtonText: button,
      cancelButtonText: t("history.cancel"),
      type: "warning",
      distinguishCancelAndClose: true
    });
    return true;
  } catch {
    return false;
  }
}

async function toggleTrash() {
  drawer.value = false;
  actions.clearFeedback();
  scrollbar.value?.setScrollTop(0);
  await history.setTrashed(!history.trashed.value);
}

async function emptyTrash() {
  if (!actions.desktop || !history.fileDeletionSupported.value || actions.busy.value || !history.trashCount.value) return;
  if (!await confirm(t("history.emptyTrashConfirm", {count: history.trashCount.value}), t("history.emptyTrashTitle"), t("history.emptyTrash"))) return;
  if (!history.fileDeletionSupported.value || actions.busy.value) return;
  if (await actions.emptyTrash()) {
    toast(t("history.trashEmptied"));
    await changed();
  } else {
    failed();
    await history.refresh();
  }
}

async function action(name: string, record: DownloadRecord) {
  if (name === "cancel") {
    if (!await tasks.cancel(record.requestId)) toast(t("download.errors.cancelFailed"), "error");
    return;
  }
  if (name === "details") {
    returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    actions.clearFeedback();
    selected.value = record;
    drawer.value = true;
    return;
  }
  if (name === "prepare") {
    if (actions.desktop && !actions.busy.value) await restartRecord(record);
    return;
  }
  if (name === "restore") {
    if (record.deletedAt && record.status !== "running" && actions.desktop) await restore(record);
    return;
  }
  if (name === "purge") {
    if (!record.deletedAt || record.status === "running" || !actions.desktop || !history.fileDeletionSupported.value || actions.busy.value) return;
    const message = h("div", [h("p", t("history.purgeConfirm")), ...(record.outputPath ? [h("p", {
      style: {
        whiteSpace: "pre-wrap",
        overflowWrap: "anywhere",
        userSelect: "text"
      }
    }, record.outputPath ?? '')] : [])]);
    if (!await confirm(message, t("history.purgeTitle"), t("history.purge"))) return;
    if (!history.fileDeletionSupported.value || actions.busy.value) return;
    if (await actions.purge(record)) {
      toast(t("history.purged"));
      await changed(record);
    } else {
      failed();
      await history.refresh();
    }
    return;
  }
  if (name === "remove") {
    if (record.deletedAt || record.status === "running" || !actions.desktop || actions.busy.value) return;
    if (!await confirm(t("history.removeConfirm"), t("history.removeTitle"), t("history.confirm"))) return;
    if (actions.busy.value) return;
    if (await actions.remove(record)) {
      toast(t("history.removed"));
      await changed(record);
    } else failed();
    return;
  }
  if (name === "removeAndFile") {
    if (record.deletedAt || record.status !== "completed" || !record.outputPath || !actions.desktop || !history.fileRecyclingSupported.value || actions.busy.value) return;
    const path = record.outputPath, name = path.split(/[\\/]/).pop() ?? path;
    const message = h("div", [
      h("p", t("history.removeAndFileConfirm")), h("p", {style: {fontWeight: "600"}}, name),
      h("p", {style: {whiteSpace: "pre-wrap", overflowWrap: "anywhere", userSelect: "text"}}, path),
      h("p", t("history.removeAndFileScope")),
    ]);
    if (!await confirm(message, t("history.removeAndFile"), t("history.removeAndFile"))) return;
    if (!history.fileRecyclingSupported.value || actions.busy.value) return;
    const result = await actions.removeAndFile(record);
    if (result) {
      toast(t(result.fileRecycled ? "history.fileAndRecordRecycled" : "history.fileMissingRecordRemoved"), result.fileRecycled ? "success" : "info");
      await changed(record);
    } else failed();
    return;
  }
  if (name === "openFile" || name === "openFolder") {
    const opened = await (name === "openFile" ? recovery.openFile(record) : recovery.openFolder(record));
    if (!opened && actions.error.value && actions.error.value.code !== "historyFileMissing") failed();
  } else if (name === "openSource") {
    if (!await actions.openSource(record)) failed();
  } else if (name === "copyLink") {
    if (actions.busy.value) return;
    const copied = await actions.copyLink(record);
    const message = t(copied ? "history.linkCopied" : actions.error.value?.code === "desktopOnly" ? "history.desktopOnly" : "download.errors.clipboardWriteFailed");
    actions.clearFeedback();
    ElMessage({showClose: true, message, type: copied ? "success" : "error", grouping: true});
  }
}
</script>
<template>
  <div class="history-container">
    <section :aria-busy="history.loading.value" aria-labelledby="page-title" class="history-page">
      <header class="history-header">
        <div class="history-toolbar">
          <ElButton v-if="history.trashed.value" :aria-label="t('history.backToHistory')" :icon="ArrowLeft"
                    @click="toggleTrash">{{ t('history.back') }}
          </ElButton>
          <div v-else :aria-label="t('navigation.history')" class="filters" role="group">
            <button v-for="status in statuses" :key="status??'all'" :aria-pressed="history.filters.value.status===status"
                    :class="{selected:history.filters.value.status===status}"
                    type="button" @click="filter(status)">
              {{ t(status ? `history.status.${status}` : 'history.all') }} ({{
                history.totalCount.value == null ? '—' : status ? (status === 'running' ? history.statusCounts.value.running + (history.statusCounts.value.queued ?? 0) : history.statusCounts.value[status]) : Object.values(history.statusCounts.value).reduce((sum, count) => sum + count, 0)
              }})
            </button>
          </div>
          <div class="heading-actions">
            <ElButton v-if="!history.trashed.value" @click="toggleTrash">{{ t('history.trash') }}</ElButton>
            <ElTooltip v-else :content="t('history.fileDeletionUnavailable')"
                       :disabled="history.fileDeletionSupported.value"><span><ElButton :disabled="!actions.desktop||!history.fileDeletionSupported.value||actions.busy.value||!history.trashCount.value" plain
                                                                                       type="danger"
                                                                                       @click="emptyTrash">{{
                t('history.emptyTrash')
              }}</ElButton></span></ElTooltip>
            <ElButton :icon="RefreshRight" :loading="history.loading.value" @click="history.refresh">
              {{ t('history.refresh') }}
            </ElButton>
          </div>
        </div>
        <div v-if="history.error.value" class="persistent-error" role="alert">{{ t('history.loadFailed') }}
          <ElButton size="small" @click="history.refresh">{{ t('persistence.retry') }}</ElButton>
        </div>
        <ElInput :aria-label="t('history.search')" :model-value="search" :placeholder="t('history.search')"
                 :prefix-icon="Search" class="history-search" clearable @update:model-value="searchChanged"/>
      </header>
      <ElScrollbar ref="scrollbar" :aria-label="t('navigation.history')" :tabindex="0" class="history-scrollbar" height="100%"
                   role="region" view-class="history-list">
        <p v-if="retainedOutsideFilter" class="action-feedback" role="status">{{ t('tasks.retryRetained') }}</p>
        <p v-if="!actions.desktop" class="action-feedback">{{ t('history.desktopOnly') }}</p>
        <ElEmpty v-if="!history.loading.value&&!history.error.value&&groups.length===0"
                 :description="t(filtered?'history.noResults':history.trashed.value?'history.trashEmpty':'pages.history.emptyTitle')">
          <p v-if="filtered&&!history.trashed.value">{{ t('history.noResultsHint') }}</p>
        </ElEmpty>
        <p v-if="history.loading.value&&!history.records.value.length" class="loading" role="status">
          {{ t('history.refresh') }}…</p>
        <section v-for="group in groups" :key="group.date" :aria-label="group.label===group.date?group.date:t(`history.${group.label}`)"
                 class="date-group"><h2>
          {{ group.label === group.date ? group.date : t(`history.${group.label}`) }}</h2>
          <div class="cards">
            <DownloadHistoryCard v-for="record in group.records" :key="record.id" :busy="actions.busy.value||tasks.isSubmitting(record.id)"
                                 :desktop="actions.desktop" :downloading="props.downloading"
                                 :file-deletion-supported="history.fileDeletionSupported.value"
                                 :record="record"
                                 :retry-error="tasks.redownloadErrors.value[record.id]"
                                 :task="taskForRecord(record)" @action="action"/>
          </div>
        </section>
        <div v-if="history.nextCursor.value" class="pagination">
          <ElButton :loading="history.loading.value" @click="history.loadMore">{{ t('history.loadMore') }}</ElButton>
        </div>
      </ElScrollbar>
    </section>
    <DownloadHistoryDetails v-model="drawer" :busy="actions.busy.value||Boolean(selected&&tasks.isSubmitting(selected.id))" :desktop="actions.desktop"
                            :downloading="props.downloading" :error="feedbackError"
                            :file-deletion-supported="history.fileDeletionSupported.value"
                            :file-recycling-supported="history.fileRecyclingSupported.value"
                            :record="displayedSelected"
                            :success="feedbackSuccess" :task="taskForRecord(displayedSelected)" @action="action" @closed="restoreFocus"/>
  </div>
</template>
<style scoped>
.history-container {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}

.history-page {
  display: flex;
  flex: 1;
  flex-direction: column;
  width: 100%;
  max-width: calc(1120px + 2 * var(--app-page-padding-x));
  margin: 0 auto;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}

.history-header {
  flex-shrink: 0;
  padding: 0 var(--app-page-padding-x) 12px;
}

.history-toolbar {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 12px 16px;
  margin-bottom: 14px;
}

.heading-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  margin-left: auto;
}

.heading-actions :deep(.el-button+.el-button) {
  margin-left: 0;
}

.filters {
  display: flex;
  gap: 7px;
  flex-wrap: wrap;
}

.filters button {
  border: 1px solid var(--app-border);
  border-radius: 7px;
  background: var(--app-surface);
  color: var(--app-text-secondary);
  padding: 7px 11px;
  font-size: 12px;
  line-height: 16px;
  white-space: nowrap;
  cursor: pointer;
}

.filters button.selected {
  background: var(--app-accent-soft);
  border-color: var(--el-color-primary-light-5);
  color: var(--app-accent);
  font-weight: 600;
}

.filters button:focus-visible {
  outline: 2px solid var(--app-accent);
  outline-offset: 2px;
}

.history-scrollbar {
  flex: 1;
  min-height: 0;
}

.history-scrollbar :deep(.history-list) {
  padding: 0 var(--app-page-padding-x) calc(var(--app-page-padding-bottom) + 20px);
}

.date-group h2 {
  font-size: 12px;
  font-weight: 600;
  color: var(--app-text-secondary);
  margin: 10px 0;
}

.date-group + .date-group {
  margin-top: 22px;
}

.cards {
  display: flex;
  flex-direction: column;
  gap: 11px;
}

.persistent-error {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
  border: 1px solid var(--el-color-danger-light-5);
  border-radius: 8px;
  padding: 12px;
  color: var(--el-color-danger);
  font-size: 13px;
  margin-bottom: 16px;
  overflow-wrap: anywhere;
}

.action-feedback, .loading {
  font-size: 13px;
  color: var(--app-text-secondary);
}

.pagination {
  display: flex;
  justify-content: center;
  margin-top: 22px;
}
</style>
