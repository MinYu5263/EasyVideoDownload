import {readFile, writeFile} from "node:fs/promises";
import {dirname, join, resolve} from "node:path";
import {fileURLToPath} from "node:url";

const requestedVersion = process.argv[2];
if (process.argv.length !== 3 || !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(requestedVersion)) {
    throw new Error("Usage: npm run version:set -- <major.minor.patch>");
}

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const packagePath = join(projectRoot, "package.json");
const tauriPath = join(projectRoot, "src-tauri", "tauri.conf.json");
const cargoPath = join(projectRoot, "src-tauri", "Cargo.toml");
const lockPath = join(projectRoot, "src-tauri", "Cargo.lock");
const [packageSource, tauriSource, cargoSource, lockSource] = await Promise.all([
    readFile(packagePath, "utf8"),
    readFile(tauriPath, "utf8"),
    readFile(cargoPath, "utf8"),
    readFile(lockPath, "utf8"),
]);

// Confirm the project version source and locate each file's version field
if (JSON.parse(tauriSource).version !== "../package.json") {
    throw new Error("Tauri must read its version from ../package.json");
}

function replaceVersion(source, pattern, fileName) {
    if ([...source.matchAll(pattern)].length !== 1) {
        throw new Error(`Expected one application version in ${fileName}`);
    }
    return source.replace(pattern, (_, prefix, _current, suffix) => `${prefix}${requestedVersion}${suffix}`);
}

const nextPackage = replaceVersion(packageSource, /^([ \t]*"version"[ \t]*:[ \t]*")([^"]+)(")/gm, "package.json");
const nextCargo = replaceVersion(cargoSource, /^(version[ \t]*=[ \t]*")([^"]+)(")/gm, "Cargo.toml");
const nextLock = replaceVersion(lockSource, /(\[\[package\]\]\r?\nname = "EasyVideoDownload"\r?\nversion = ")([^"]+)(")/g, "Cargo.lock");

// Keep the frontend version, Rust package, and lockfile in sync
for (const [path, current, updated] of [
    [packagePath, packageSource, nextPackage],
    [cargoPath, cargoSource, nextCargo],
    [lockPath, lockSource, nextLock],
]) {
    if (current !== updated) await writeFile(path, updated);
}

console.log(`Application version: ${requestedVersion}`);
