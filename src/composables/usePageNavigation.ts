import {computed} from "vue";
import type {AppPageId, PersistedAppPageId} from "../navigation.ts";

export function createPageNavigation(preferences: {
    draft: { activePage: PersistedAppPageId };
    update: (patch: { activePage: PersistedAppPageId }) => Promise<boolean>
}) {
    const activePageId = computed<AppPageId>({
        get: () => preferences.draft.activePage,
        set: value => {
            void preferences.update({activePage: value});
        }
    });
    return {activePageId};
}
