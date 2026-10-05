import {computed, ref} from "vue";
import type {AppPageId, PersistedAppPageId} from "../navigation.ts";

export function createPageNavigation(preferences: {
    draft: { activePage: PersistedAppPageId };
    update: (patch: { activePage: PersistedAppPageId }) => Promise<boolean>
}) {
    const labVisited = ref(false), experiment = ref(false);
    const activePageId = computed<AppPageId>({
        get: () => experiment.value ? "douyin-lab" : preferences.draft.activePage, set: value => {
            experiment.value = value === "douyin-lab";
            if (value === "douyin-lab") labVisited.value = true;
            else void preferences.update({activePage: value});
        }
    });
    return {activePageId, labVisited};
}
