import {mkdir, mkdtemp, readFile, realpath, rm, writeFile} from "node:fs/promises";
import {dirname, isAbsolute, join, relative, resolve, sep} from "node:path";
import {fileURLToPath} from "node:url";
import {createHash} from "node:crypto";
import {spawn} from "node:child_process";
import {createRequire} from "node:module";

const tauriCli = createRequire(import.meta.url).resolve("@tauri-apps/cli/tauri.js");
// The native CLI initializes a process-wide logger, so each invocation needs
// its own process when generating both the standard and macOS icon sets.
function generateIcons(source, output) {
    return new Promise((resolve, reject) => {
        const child = spawn(process.execPath, [tauriCli, "icon", source, "--output", output], {stdio: "inherit"});
        child.once("error", reject);
        child.once("close", (code, signal) => {
            if (code === 0) resolve();
            else reject(new Error(`Icon generation failed (${signal ?? code}): ${source}`));
        });
    });
}

const projectRoot = await realpath(resolve(dirname(fileURLToPath(import.meta.url)), ".."));
const sourceIcon = join(projectRoot, "src", "assets", "app-icon.svg");
const targetDirectory = join(projectRoot, "src-tauri", "target");
const iconDirectory = join(targetDirectory, "generated-icons");
const iconFiles = ["32x32.png", "128x128.png", "128x128@2x.png", "icon.png", "icon.ico", "icon.icns", "macos-icon.png"];
const sourceBytes = await readFile(sourceIcon);
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
const sourceDigest = digest(Buffer.concat([
    sourceBytes,
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
    console.log("Desktop icons, including the macOS variant, are up to date.");
} else {
    const temporaryDirectory = await mkdtemp(join(resolvedTarget, "icon-generation-"));
    console.log(`Generating desktop icons from src/assets/app-icon.svg (temporary output: ${temporaryDirectory})`);
    try {
        // Derive a macOS-only tile: 824px artwork on a 1024px canvas, with
        // 185px rounded corners. Keep the source SVG and other platforms intact.
        // This adapts legacy ICNS geometry; Tahoe's final presentation must still
        // be checked in Finder/Dock rather than inferred from this PNG preview.
        const sourceSvg = sourceBytes.toString("utf8");
        const background = /<rect\s+x="12"\s+y="12"\s+width="232"\s+height="232"\s+rx="24"\s+fill="([^"]+)"\s*\/>/g;
        const matches = [...sourceSvg.matchAll(background)];
        if (matches.length !== 1 || !sourceSvg.includes('viewBox="0 0 256 256"')) {
            throw new Error("App icon geometry changed; update the macOS icon adaptation before generating icons.");
        }
        const scale = 206 / 232;
        const macosSvg = sourceSvg.replace(background, (_, fill) =>
            `<rect x="25" y="25" width="206" height="206" rx="46.25" fill="${fill}"/>\n` +
            `<g transform="translate(128 128) scale(${scale}) translate(-128 -128)">`
        ).replace("</svg>", "</g>\n</svg>");
        const macosSource = join(temporaryDirectory, "macos.svg");
        const macosOutput = join(temporaryDirectory, "macos");
        await writeFile(macosSource, macosSvg);
        await generateIcons(sourceIcon, temporaryDirectory);
        await generateIcons(macosSource, macosOutput);
        await mkdir(iconDirectory, {recursive: true});
        const files = {};
        for (const name of iconFiles) {
            const generatedPath = name === "icon.icns" ? join(macosOutput, name)
                : name === "macos-icon.png" ? join(macosOutput, "icon.png")
                : join(temporaryDirectory, name);
            const generated = await readFile(generatedPath);
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
