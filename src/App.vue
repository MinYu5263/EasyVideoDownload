<script setup lang="ts">
import {computed, ref} from "vue";
import {ElConfigProvider, ElEmpty, ElIcon, ElScrollbar} from "element-plus";
import {useI18n} from "vue-i18n";
import AppSidebar from "./components/AppSidebar.vue";
import DownloadPage from "./components/DownloadPage.vue";
import SettingsPage from "./components/SettingsPage.vue";
import {elementPlusLocale} from "./i18n";
import {type AppPageId, appPages} from "./navigation";

const activePageId = ref<AppPageId>("download");
const {t} = useI18n({useScope: "global"});
const activePage = computed(
    () => appPages.find((page) => page.id === activePageId.value) ?? appPages[0],
);
</script>

<template>
  <ElConfigProvider :locale="elementPlusLocale">
    <a class="skip-link" href="#main-content">
      {{ t("accessibility.skipToContent") }}
    </a>

    <div class="app-shell">
      <AppSidebar v-model="activePageId"/>

      <main id="main-content" class="app-main" tabindex="-1">
        <header class="page-heading">
          <h1 id="page-title">{{ t(activePage.labelKey) }}</h1>
        </header>

        <DownloadPage v-show="activePage.id === 'download'" :active="activePage.id === 'download'"/>
        <SettingsPage v-show="activePage.id === 'settings'"/>
        <ElScrollbar
            v-show="activePage.id === 'history'"
            :aria-label="t(activePage.labelKey)"
            :tabindex="0"
            class="page-scrollbar"
            height="100%"
            role="region"
            view-class="page-content"
        >
          <section
              v-if="activePage.id === 'history'"
              :key="activePage.id"
              aria-labelledby="page-title"
              class="page-placeholder"
          >
            <ElEmpty :image-size="56">
              <template #image>
                <span aria-hidden="true" class="placeholder-icon">
                  <ElIcon :size="24">
                    <component :is="activePage.icon"/>
                  </ElIcon>
                </span>
              </template>
              <template #description>
                <h2>{{ t(activePage.emptyTitleKey) }}</h2>
                <p>{{ t(activePage.emptyDescriptionKey) }}</p>
              </template>
            </ElEmpty>
          </section>
        </ElScrollbar>
      </main>
    </div>
  </ElConfigProvider>
</template>

<style scoped>
.skip-link {
  position: fixed;
  top: 12px;
  left: 12px;
  z-index: 10;
  padding: 10px 16px;
  border-radius: 8px;
  background: var(--app-accent);
  color: #ffffff;
  transform: translateY(-160%);
}

.skip-link:focus {
  transform: translateY(0);
}

.app-shell {
  display: grid;
  grid-template-columns: var(--app-sidebar-width) minmax(0, 1fr);
  height: 100%;
  overflow: hidden;
}

.app-main {
  --app-page-padding-x: 32px;
  --app-page-padding-bottom: 26px;

  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
  padding-top: 30px;
}

.app-main:focus {
  outline: none;
}

.page-heading {
  flex-shrink: 0;
  margin: 0 var(--app-page-padding-x) 24px;
}

.page-scrollbar {
  flex: 1;
  min-height: 0;
}

.page-scrollbar :deep(.page-content) {
  padding: 0 var(--app-page-padding-x) var(--app-page-padding-bottom);
}

.page-heading h1 {
  margin: 0;
  color: var(--app-text);
  font-size: 24px;
  font-weight: 650;
  line-height: 1.4;
  letter-spacing: -0.6px;
}

.page-placeholder {
  display: grid;
  min-height: 280px;
  place-items: center;
  padding: 24px;
  border: 1px dashed var(--app-border);
  border-radius: var(--app-radius);
}

.placeholder-icon {
  display: grid;
  width: 56px;
  height: 56px;
  place-items: center;
  border-radius: 50%;
  background: var(--app-accent-soft);
  color: var(--app-accent);
}

.placeholder-icon :deep(svg) {
  color: var(--app-accent);
}

.page-placeholder h2 {
  margin: 0 0 8px;
  color: var(--app-text-secondary);
  font-size: 16px;
  font-weight: 500;
}

.page-placeholder p {
  max-width: 360px;
  margin: 0;
  color: var(--app-text-muted);
  font-size: 13px;
  line-height: 1.7;
}

@media (max-width: 1000px) {
  .app-main {
    --app-page-padding-x: 24px;
    --app-page-padding-bottom: 24px;

    padding-top: 27px;
  }
}
</style>
