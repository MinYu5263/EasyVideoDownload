import {ref} from "vue";

// The parent handles persistence; this editor handles file and clipboard input.
export function createCookieEditor(
    initialText: string,
    onChange: (contents: string) => void,
) {
    const text = ref(initialText);
    const reading = ref(false);
    let readVersion = 0;

    function cancelPendingRead() {
        readVersion += 1;
        reading.value = false;
    }

    function setText(value: string, force = false) {
        cancelPendingRead();
        if (text.value === value && !force) return;
        text.value = value;
        onChange(value);
    }

    function restoreText(value: string) {
        cancelPendingRead();
        text.value = value;
    }

    async function readContents(read: () => Promise<string | null>) {
        const version = ++readVersion;
        reading.value = true;
        try {
            const contents = await read();
            if (version === readVersion && contents !== null) setText(contents, true);
        } catch (error) {
            if (version === readVersion) throw error;
        } finally {
            if (version === readVersion) reading.value = false;
        }
    }

    function importContents(read: () => Promise<string | null>) {
        return readContents(read);
    }

    function readClipboard(read: () => Promise<string>) {
        return readContents(read);
    }

    function clear() {
        setText("", true);
    }

    return {text, reading, setText, restoreText, importContents, readClipboard, clear, dispose: cancelPendingRead};
}
