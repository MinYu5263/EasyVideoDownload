<script lang="ts" setup>
import {KeepAlive, nextTick, onUnmounted, ref, watch} from "vue";
import ContentMotion from "./ContentMotion.vue";
import HistoryScopePage from "./HistoryScopePage.vue";
import {useDownloadHistory} from "../composables/useDownloadHistory";
import {useDownloadHistoryActions} from "../composables/downloadHistoryActions";
import {useI18n} from "vue-i18n";
import {useFeedback} from "../composables/useFeedback";

const props = withDefaults(defineProps<{ active: boolean; downloading?: boolean }>(), {downloading: false});
const history = useDownloadHistory(), trash = useDownloadHistory(true);
// Keep native operations serial across both retained scopes
const actions = useDownloadHistoryActions();
const {t} = useI18n({useScope: "global"});
const {watchError} = useFeedback();
watchError(trash.connectionError, () => t('history.listenFailed'), () => ({key: 'history:trash:connection'}));
const trashed = ref(false);
const scopeMotion = ref<InstanceType<typeof ContentMotion>>();
const view = ref<InstanceType<typeof HistoryScopePage>>();
let navigation = 0;

function waitForTransition() {
  return scopeMotion.value?.whenIdle() ?? Promise.resolve();
}

async function switchScope(value: boolean) {
  // Select the retained view before its background query completes
  const current = ++navigation;
  trashed.value = value;
  await nextTick();
  await waitForTransition();
  if (props.active && current === navigation) view.value?.focusScopeButton();
}

async function revealRecord(id: number, targetTrashed = false) {
  const current = ++navigation;
  trashed.value = targetTrashed;
  await nextTick();
  if (props.active && current === navigation) await view.value?.revealRecord(id, targetTrashed);
}

watch(() => props.active, active => {
  navigation++;
  if (active) trashed.value = false;
});
onUnmounted(() => {
  navigation++;
  trash.dispose();
});
defineExpose({revealRecord});
</script>

<template>
  <div class="history-container">
    <ContentMotion ref="scopeMotion" :active="props.active" :position="trashed ? 1 : 0" :view-key="trashed">
      <KeepAlive :max="2">
        <HistoryScopePage :key="trashed ? 'trash' : 'history'" ref="view" :actions="actions" :active="props.active"
                          :downloading="props.downloading" :history="trashed ? trash : history"
                          :wait-for-transition="waitForTransition" @switch-scope="switchScope"/>
      </KeepAlive>
    </ContentMotion>
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
</style>
