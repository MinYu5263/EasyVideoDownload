import {ref} from 'vue';
import type {DownloadTaskSnapshot} from './downloadTaskTypes';

export interface TaskCardClock {
    setTimeout(callback: () => void, milliseconds: number): number;

    clearTimeout(handle: number): void
}

export function createTaskCardVisibility({clock}: { clock: TaskCardClock }) {
    const hidden = ref(new Set<number>()), tasks = new Map<number, DownloadTaskSnapshot>(),
        timers = new Map<number, number>(), interactions = new Set<number>(), retryHolds = new Set<number>();
    let visible = true;
    const terminal = (task: DownloadTaskSnapshot) => !task.storageError && ['completed', 'cancelled'].includes(task.phase);

    function stop(id: number) {
        const timer = timers.get(id);
        if (timer !== undefined) clock.clearTimeout(timer);
        timers.delete(id);
    }

    function schedule(id: number) {
        const task = tasks.get(id);
        if (!task || !visible || (interactions.has(id) || retryHolds.has(id)) || hidden.value.has(id) || !terminal(task) || timers.has(id)) return;
        const uuid = task.record.requestId;
        timers.set(id, clock.setTimeout(() => {
            timers.delete(id);
            const latest = tasks.get(id);
            if (latest?.record.requestId === uuid && terminal(latest) && visible && !interactions.has(id) && !retryHolds.has(id)) hidden.value = new Set([...hidden.value, id]);
        }, 3000));
    }

    function update(task: DownloadTaskSnapshot) {
        const id = task.record.id, previous = tasks.get(id);
        if (previous?.record.requestId !== task.record.requestId || task.storageError) {
            stop(id);
            const next = new Set(hidden.value);
            next.delete(id);
            hidden.value = next;
        }
        tasks.set(id, task);
        if (!terminal(task)) stop(id);
        schedule(id);
    }

    function setVisible(value: boolean) {
        if (visible === value) return;
        visible = value;
        for (const id of tasks.keys()) {
            stop(id);
            if (value) schedule(id);
        }
    }

    function setInteracting(id: number, value: boolean) {
        if (value) {
            interactions.add(id);
            stop(id);
        } else {
            interactions.delete(id);
            schedule(id);
        }
    }

    function setRetryHeld(id: number, value: boolean) {
        if (value) {
            retryHolds.add(id);
            stop(id);
            const next = new Set(hidden.value);
            next.delete(id);
            hidden.value = next;
        } else {
            retryHolds.delete(id);
            schedule(id);
        }
    }

    function dismiss(id: number) {
        stop(id);
        hidden.value = new Set([...hidden.value, id]);
    }

    function dispose() {
        for (const id of timers.keys()) stop(id);
        tasks.clear();
        interactions.clear();
        retryHolds.clear();
    }

    return {
        hidden,
        update,
        setVisible,
        setInteracting,
        setRetryHeld,
        dismiss,
        isHidden: (id: number) => hidden.value.has(id),
        dispose
    };
}
