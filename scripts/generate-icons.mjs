import {mkdir, mkdtemp, readFile, realpath, rm, writeFile} from "node:fs/promises";
import {dirname, isAbsolute, join, relative, resolve, sep} from "node:path";
import {fileURLToPath} from "node:url";
import {createHash} from "node:crypto";
import {run} from "@tauri-apps/cli";

const projectRoot = await realpath(resolve(dirname(fileURLToPath(import.meta.url)), ".."));
const sourceIcon = join(projectRoot, "src", "assets", "app-icon.svg");
const targetDirectory = join(projectRoot, "src-tauri", "target");
const iconDirectory = join(targetDirectory, "generated-icons");
const iconFiles = ["32x32.png", "128x128.png", "128x128@2x.png", "icon.png", "icon.ico", "icon.icns"];
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
const sourceDigest = digest(Buffer.concat([
    await readFile(sourceIcon),
    await readFile(fileURLToPath(import.meta.url)),
]));

await mkdir(targetDirectory, {recursive: true});
const resolvedTarget = await realpath(targetDirectory);
const targetRelative = relative(projectRoot, resolvedTarget);
if (isAbsolute(targetRelative) || targetRelative === ".." || targetRelative.startsWith(`..${sep}`)) {
    throw new Error("Icon generation directory must stay inside the project.");
}

// Cache source and output hashes in the build directory. ICNS chunk ordering can vary
// between CLI runs, so unchanged sources should reuse verified generated files.
const cachePath = join(resolvedTarget, ".app-icon-generation.json");
let cache;
try {
    cache = JSON.parse(await readFile(cachePath, "utf8"));
} catch (error) {
    if (error.code !== "ENOENT" && !(error instanceof SyntaxError)) throw error;
}
let upToDate = cache?.source === sourceDigest;
if (upToDate) {
    for (const name of iconFiles) {
        try {
            if (cache.files?.[name] !== digest(await readFile(join(iconDirectory, name)))) upToDate = false;
        } catch (error) {
            if (error.code !== "ENOENT") throw error;
            upToDate = false;
        }
    }
}

if (upToDate) {
    console.log("Desktop icons match src/assets/app-icon.svg.");
} else {
    const temporaryDirectory = await mkdtemp(join(resolvedTarget, "icon-generation-"));
    console.log(`Generating desktop icons from src/assets/app-icon.svg (temporary output: ${temporaryDirectory})`);
    try {
        await run(["icon", sourceIcon, "--output", temporaryDirectory]);
        await mkdir(iconDirectory, {recursive: true});
        const files = {};
        for (const name of iconFiles) {
            const generated = await readFile(join(temporaryDirectory, name));
            const destination = join(iconDirectory, name);
            let previous;
            try {
                previous = await readFile(destination);
            } catch (error) {
                if (error.code !== "ENOENT") throw error;
            }
            if (!previous?.equals(generated)) await writeFile(destination, generated);
            files[name] = digest(generated);
        }
        await writeFile(cachePath, JSON.stringify({source: sourceDigest, files}, null, 2) + "\n");
    } finally {
        const resolvedTemporary = await realpath(temporaryDirectory);
        if (dirname(resolvedTemporary) !== resolvedTarget || !resolvedTemporary.startsWith(join(resolvedTarget, "icon-generation-"))) {
            throw new Error(`Unexpected icon cleanup path: ${resolvedTemporary}`);
        }
        await rm(resolvedTemporary, {recursive: true, force: true});
    }
}
