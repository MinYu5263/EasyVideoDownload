<script setup lang="ts">
import {computed, ref} from "vue";
import {ElConfigProvider, ElEmpty, ElIcon} from "element-plus";
import {useI18n} from "vue-i18n";
import AppSidebar from "./components/AppSidebar.vue";
import SettingsPage from "./components/SettingsPage.vue";
import {elementPlusLocale} from "./i18n";
import {appPages, type AppPageId} from "./navigation";

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

        <SettingsPage v-show="activePage.id === 'settings'"/>
        <section
            v-if="activePage.id !== 'settings'"
            :key="activePage.id"
            class="page-placeholder"
            aria-labelledby="page-title"
        >
          <ElEmpty :image-size="56">
            <template #image>
              <span class="placeholder-icon" aria-hidden="true">
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
  min-width: 0;
  overflow-y: auto;
  padding: 30px 32px 26px;
}

.app-main:focus {
  outline: none;
}

.page-heading {
  margin-bottom: 24px;
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
    padding: 27px 24px 24px;
  }
}
</style>
