<script setup lang="ts">
import {ref, watch} from "vue";
import {useI18n} from "vue-i18n";
import {ElIcon, ElMenu, ElMenuItem} from "element-plus";
import {type AppPageId, appPages} from "../navigation";
import appIcon from "../assets/app-icon.svg?no-inline";

const props = defineProps<{ modelValue: AppPageId }>();

const {t} = useI18n({useScope: "global"});

const focusedPageId = ref<AppPageId>(props.modelValue);
watch(
    () => props.modelValue,
    (pageId) => {
      focusedPageId.value = pageId;
    },
);

const emit = defineEmits<{
  "update:modelValue": [pageId: AppPageId];
}>();

function selectPage(index: string) {
  const page = appPages.find((item) => item.id === index);
  if (page) emit("update:modelValue", page.id);
}

function handleMenuKeydown(event: KeyboardEvent) {
  const menu = event.currentTarget as HTMLElement;
  const items = Array.from(
      menu.querySelectorAll<HTMLElement>('[role="menuitem"]'),
  );
  const currentIndex = items.indexOf(document.activeElement as HTMLElement);
  if (currentIndex < 0) return;

  let nextIndex: number;
  switch (event.key) {
    case "ArrowDown":
      nextIndex = (currentIndex + 1) % items.length;
      break;
    case "ArrowUp":
      nextIndex = (currentIndex - 1 + items.length) % items.length;
      break;
    case "Home":
      nextIndex = 0;
      break;
    case "End":
      nextIndex = items.length - 1;
      break;
    case "Enter":
    case " ":
      event.preventDefault();
      items[currentIndex].click();
      return;
    default:
      return;
  }

  event.preventDefault();
  items[nextIndex].focus();
}
</script>

<template>
  <aside class="app-sidebar" :aria-label="t('accessibility.sidebar')">
    <div class="brand">
      <img :src="appIcon" alt="" aria-hidden="true" class="brand-icon" height="32" width="32"/>
      <strong>EasyVideoDownload</strong>
    </div>

    <nav :aria-label="t('accessibility.mainNavigation')">
      <ElMenu
          class="sidebar-menu"
          role="menu"
          :aria-label="t('accessibility.pageNavigation')"
          aria-orientation="vertical"
          :default-active="modelValue"
          @select="selectPage"
          @keydown="handleMenuKeydown"
      >
        <ElMenuItem
            v-for="page in appPages"
            :key="page.id"
            :index="page.id"
            :aria-current="modelValue === page.id ? 'page' : undefined"
            :tabindex="focusedPageId === page.id ? 0 : -1"
            @focus="focusedPageId = page.id"
        >
          <ElIcon :size="18" aria-hidden="true">
            <component :is="page.icon"/>
          </ElIcon>
          <span>{{ t(page.labelKey) }}</span>
        </ElMenuItem>
      </ElMenu>
    </nav>
  </aside>
</template>

<style scoped>
.app-sidebar {
  display: flex;
  min-height: 0;
  flex-direction: column;
  overflow-y: auto;
  padding: 28px 16px 22px;
  border-right: 1px solid var(--app-border);
  background: var(--app-sidebar-background);
}

.brand {
  display: flex;
  flex-shrink: 0;
  align-items: center;
  gap: 9px;
  margin-bottom: 30px;
}

.brand-icon {
  display: block;
  width: 32px;
  height: 32px;
  flex-shrink: 0;
}

.brand strong {
  font-size: 14px;
  font-weight: 650;
  letter-spacing: -0.45px;
  white-space: nowrap;
}

.sidebar-menu {
  --el-menu-bg-color: transparent;
  --el-menu-text-color: var(--app-text-secondary);
  --el-menu-active-color: var(--app-accent);
  --el-menu-hover-bg-color: var(--app-hover);
  --el-menu-item-height: 44px;
  --el-menu-base-level-padding: 14px;

  display: flex;
  flex-direction: column;
  gap: 6px;
  border-right: none;
}

.sidebar-menu :deep(.el-menu-item) {
  border-radius: 9px;
  font-size: 13px;
}

.sidebar-menu :deep(.el-menu-item .el-icon) {
  width: 18px;
  margin-right: 12px;
}

.sidebar-menu :deep(.el-menu-item.is-active) {
  background: var(--app-accent-soft);
  font-weight: 600;
}

.sidebar-menu :deep(.el-menu-item:focus-visible) {
  outline: 2px solid var(--app-accent);
  outline-offset: -2px;
}

@media (max-width: 1000px) {
  .app-sidebar {
    padding-right: 12px;
    padding-left: 12px;
  }

  .brand {
    gap: 8px;
  }

  .brand strong {
    font-size: 13px;
  }
}
</style>
