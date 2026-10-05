import {computed, onMounted, onUnmounted, reactive, ref, watch} from "vue";
import {invoke, isTauri} from "@tauri-apps/api/core";

export type ProxyProtocol = "http" | "https" | "socks5";

export interface ProxySettings {
    protocol: ProxyProtocol;
    address: string;
    port: number
}

export interface ProxyDraft {
    protocol: ProxyProtocol;
    address: string;
    port: string
}

interface ProxyError {
    code: string;
    detail?: string
}

interface ProxyTestResult {
    target: string;
    status: number;
    elapsedMs: number
}

interface ProxyBridge {
    desktop: boolean;
    invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>
}

export const proxySettingsRevision = ref(0);

const ipv4Address = /^(?:(?:25[0-5]|2[0-4]\d|1\d{2}|[1-9]?\d)\.){3}(?:25[0-5]|2[0-4]\d|1\d{2}|[1-9]?\d)$/;
const domainAddress = /^(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z](?:[a-z0-9-]{0,61}[a-z0-9])?\.?$/i;

function normalizedAddress(input: string): string | undefined {
    const text = input.trim();
    if (!text || /[\s/\\?#@%]/.test(text)) return;
    if (text.startsWith("[") && !text.endsWith("]")) return;
    try {
        const host = text.includes(":") && !text.startsWith("[") ? `[${text}]` : text;
        const url = new URL(`http://${host}`);
        const address = url.hostname;
        if (!address.startsWith("[")) {
            // URL parsing accepts numeric shortcuts such as 127.1 and 0x7f000001.
            // Require the original input to be a complete decimal IPv4 address.
            if (ipv4Address.test(address)) {
                if (!ipv4Address.test(text)) return;
            } else if (address.replace(/\.$/, "").length > 253 ||
                (address !== "localhost" && address !== "localhost." && !domainAddress.test(address))) return;
        }
        return address.startsWith("[") ? address.slice(1, -1) : address;
    } catch {
        return;
    }
}

export function proxyCandidate(draft: ProxyDraft): ProxySettings | null | undefined {
    if (!draft.address.trim() && !draft.port) return null;
    const address = normalizedAddress(draft.address);
    const port = Number(draft.port);
    if (!address || !/^\d+$/.test(draft.port) || port < 1 || port > 65535 ||
        !["http", "https", "socks5"].includes(draft.protocol)) return;
    return {protocol: draft.protocol, address, port};
}

function configurationKey(settings: ProxySettings | null) {
    return JSON.stringify(settings);
}

function failure(error: unknown): ProxyError {
    return typeof error === "object" && error !== null && "code" in error
        ? error as ProxyError : {code: "bridgeFailed"};
}

export function createProxySettings(bridge: ProxyBridge) {
    const draft = reactive<ProxyDraft>({protocol: "http", address: "", port: ""});
    const saved = ref<ProxySettings | null>(null);
    const ready = ref(false), loading = ref(false), saving = ref(false), testing = ref(false);
    const loadError = ref<ProxyError | null>(null), saveError = ref<ProxyError | null>(null),
        testError = ref<ProxyError | null>(null);
    const testResult = ref<ProxyTestResult | null>(null);
    const candidate = computed(() => proxyCandidate(draft));
    const dirty = computed(() => candidate.value === undefined || configurationKey(candidate.value) !== configurationKey(saved.value));
    const addressInvalid = computed(() => (draft.address.trim() !== "" || draft.port !== "") && !normalizedAddress(draft.address));
    const canSave = computed(() => bridge.desktop && ready.value && !saving.value && !testing.value &&
        candidate.value !== undefined && (candidate.value !== null || dirty.value));
    const canTest = computed(() => bridge.desktop && ready.value && !testing.value && !saving.value && Boolean(candidate.value));
    const proxyAddress = computed(() => {
        const settings = candidate.value;
        if (!settings) return "";
        const scheme = settings.protocol === "socks5" ? "socks5h" : settings.protocol;
        const host = settings.address.includes(":") ? `[${settings.address}]` : settings.address;
        return `${scheme}://${host}:${settings.port}`;
    });
    let revision = 0, disposed = false;
    const stop = watch(() => [draft.protocol, draft.address, draft.port], () => {
        revision++;
        testResult.value = testError.value = saveError.value = null;
    }, {flush: "sync"});

    async function load() {
        if (loading.value || disposed) return;
        loading.value = true;
        ready.value = false;
        try {
            const settings = bridge.desktop ? await bridge.invoke<ProxySettings | null>("get_proxy_settings_for_editing") : null;
            if (disposed) return;
            saved.value = settings;
            Object.assign(draft, settings ? {...settings, port: String(settings.port)} : {
                protocol: "http",
                address: "",
                port: ""
            });
            loadError.value = null;
            ready.value = true;
        } catch (error) {
            if (!disposed) loadError.value = failure(error);
        } finally {
            loading.value = false;
        }
    }

    async function persist(snapshot: ProxySettings | null) {
        if (saving.value || disposed) return false;
        saving.value = true;
        saveError.value = null;
        try {
            const settings = await bridge.invoke<ProxySettings | null>("save_proxy_settings", {settings: snapshot});
            if (disposed) return false;
            if (configurationKey(saved.value) !== configurationKey(settings)) proxySettingsRevision.value++;
            saved.value = settings;
            return true;
        } catch (error) {
            if (!disposed) saveError.value = failure(error);
            return false;
        } finally {
            saving.value = false;
        }
    }

    async function save() {
        if (!canSave.value || disposed) return false;
        return persist(candidate.value!);
    }

    async function testConnection() {
        if (!canTest.value || disposed) return;
        const current = revision;
        const settings = candidate.value!;
        testing.value = true;
        testResult.value = testError.value = null;
        try {
            const result = await bridge.invoke<ProxyTestResult>("test_proxy_connection", {settings});
            if (!disposed && current === revision) {
                testResult.value = result;
                await persist(settings);
            }
        } catch (error) {
            if (!disposed && current === revision) testError.value = failure(error);
        } finally {
            testing.value = false;
        }
    }

    function dispose() {
        disposed = true;
        revision++;
        stop();
    }

    return {
        desktop: bridge.desktop, draft, saved, ready, loading, saving, testing, dirty, addressInvalid,
        canSave, canTest, proxyAddress, loadError, saveError, testError, testResult, load, save, testConnection, dispose
    };
}

export function useProxySettings() {
    const controller = createProxySettings({desktop: isTauri(), invoke});
    onMounted(controller.load);
    onUnmounted(controller.dispose);
    return controller;
}
