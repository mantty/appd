const files = globalThis.__appd_tmp ??= new Map();
const directories = globalThis.__appd_tmp_directories ??= new Set(["/tmp"]);
const descriptors = new Map();
let nextDescriptor = 3;

function pathOf(input) {
  let path = String(input);
  if (!path.startsWith("/")) path = `${globalThis.process?.cwd?.() ?? "/"}/${path}`;
  const parts = [];
  for (const part of path.split("/")) {
    if (!part || part === ".") continue;
    if (part === "..") {
      if (!parts.length) throw new Error("EINVAL: path escapes the virtual filesystem");
      parts.pop();
    } else {
      parts.push(part);
    }
  }
  const normalized = `/${parts.join("/")}`;
  if (!normalized.startsWith("/tmp") || (normalized.length > 4 && normalized[4] !== "/")) {
    throw new Error(`ENOENT: no such file or directory, open '${normalized}'`);
  }
  return normalized;
}

function parentOf(path) {
  const index = path.lastIndexOf("/");
  return index > 0 ? path.slice(0, index) : "/";
}

function bytesOf(value, encoding) {
  if (value instanceof Uint8Array) return new Uint8Array(value);
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  if (encoding === "base64") return new TextEncoder().encode(String(value));
  return new TextEncoder().encode(String(value));
}

function textOf(value, encoding) {
  if (encoding === "buffer" || encoding == null) return new Uint8Array(value);
  return new TextDecoder().decode(value);
}

function optionsOf(options) {
  return typeof options === "string" ? { encoding: options } : options ?? {};
}

function ensureParent(path) {
  const parent = parentOf(path);
  if (!directories.has(parent)) throw new Error(`ENOENT: no such file or directory, open '${path}'`);
}

function stat(path) {
  const isFile = files.has(path);
  const isDirectory = directories.has(path);
  if (!isFile && !isDirectory) throw new Error(`ENOENT: no such file or directory, stat '${path}'`);
  const size = isFile ? files.get(path).byteLength : 0;
  return {
    size,
    mode: isFile ? 0o100666 : 0o40777,
    mtimeMs: 0,
    ctimeMs: 0,
    birthtimeMs: 0,
    isFile: () => isFile,
    isDirectory: () => isDirectory,
    isSymbolicLink: () => false,
  };
}

export function readFileSync(input, options) {
  const path = pathOf(input);
  const data = files.get(path);
  if (!data) throw new Error(`ENOENT: no such file or directory, open '${path}'`);
  return textOf(data, optionsOf(options).encoding);
}

export function writeFileSync(input, data, options) {
  const path = pathOf(input);
  ensureParent(path);
  files.set(path, bytesOf(data, optionsOf(options).encoding));
}

export function appendFileSync(input, data, options) {
  const path = pathOf(input);
  ensureParent(path);
  const previous = files.get(path) ?? new Uint8Array();
  const next = bytesOf(data, optionsOf(options).encoding);
  const result = new Uint8Array(previous.length + next.length);
  result.set(previous);
  result.set(next, previous.length);
  files.set(path, result);
}

export function mkdirSync(input, options) {
  const path = pathOf(input);
  const recursive = optionsOf(options).recursive === true;
  if (directories.has(path)) return;
  if (!recursive) ensureParent(path);
  const parts = path.split("/");
  let current = "";
  for (const part of parts) {
    if (!part) continue;
    current += `/${part}`;
    directories.add(current);
  }
}

export function readdirSync(input, options) {
  const path = pathOf(input);
  if (!directories.has(path)) throw new Error(`ENOTDIR: not a directory, scandir '${path}'`);
  const names = new Set();
  const prefix = path === "/" ? "/" : `${path}/`;
  for (const candidate of [...directories, ...files.keys()]) {
    if (!candidate.startsWith(prefix)) continue;
    const name = candidate.slice(prefix.length).split("/")[0];
    if (name) names.add(name);
  }
  const values = [...names].sort();
  return optionsOf(options).withFileTypes ? values.map((name) => ({ name, isFile: () => files.has(`${prefix}${name}`), isDirectory: () => directories.has(`${prefix}${name}`) })) : values;
}

export function statSync(input) { return stat(pathOf(input)); }
export const lstatSync = statSync;
export function existsSync(input) { try { stat(pathOf(input)); return true; } catch { return false; } }

export function unlinkSync(input) {
  const path = pathOf(input);
  if (!files.delete(path)) throw new Error(`ENOENT: no such file or directory, unlink '${path}'`);
}

export function rmSync(input, options) {
  const path = pathOf(input);
  const recursive = optionsOf(options).recursive === true;
  if (files.delete(path)) return;
  if (!directories.has(path)) {
    if (optionsOf(options).force) return;
    throw new Error(`ENOENT: no such file or directory, remove '${path}'`);
  }
  const children = [...files.keys(), ...directories].filter((candidate) => candidate.startsWith(`${path}/`));
  if (children.length && !recursive) throw new Error(`ENOTEMPTY: directory not empty, remove '${path}'`);
  for (const child of children) {
    files.delete(child);
    directories.delete(child);
  }
  directories.delete(path);
}

export const rmdirSync = rmSync;

export function renameSync(from, to) {
  const source = pathOf(from);
  const destination = pathOf(to);
  ensureParent(destination);
  if (files.has(source)) {
    files.set(destination, files.get(source));
    files.delete(source);
    return;
  }
  if (directories.has(source)) {
    directories.add(destination);
    for (const [path, value] of files) if (path.startsWith(`${source}/`)) {
      files.set(`${destination}${path.slice(source.length)}`, value);
      files.delete(path);
    }
    directories.delete(source);
    return;
  }
  throw new Error(`ENOENT: no such file or directory, rename '${source}'`);
}

export function copyFileSync(from, to) {
  const source = pathOf(from);
  const destination = pathOf(to);
  ensureParent(destination);
  const value = files.get(source);
  if (!value) throw new Error(`ENOENT: no such file or directory, copyfile '${source}'`);
  files.set(destination, new Uint8Array(value));
}

export function realpathSync(input) { return pathOf(input); }
export function openSync(input, flags = "r") {
  const path = pathOf(input);
  if (flags !== "r" && flags !== "rs") ensureParent(path);
  if (!files.has(path) && flags === "r") throw new Error(`ENOENT: no such file or directory, open '${path}'`);
  if (!files.has(path)) files.set(path, new Uint8Array());
  const descriptor = nextDescriptor++;
  descriptors.set(descriptor, path);
  return descriptor;
}
export function closeSync(descriptor) { descriptors.delete(descriptor); }

export const constants = { O_RDONLY: 0, O_WRONLY: 1, O_RDWR: 2, O_CREAT: 64, O_TRUNC: 512, O_APPEND: 1024 };

function callbackForm(sync, args) {
  const callback = args.at(-1);
  const values = args.slice(0, -1);
  try { callback(null, sync(...values)); } catch (error) { callback(error); }
}

export function readFile(input, options, callback) {
  if (typeof options === "function") return callbackForm(readFileSync, [input, options]);
  return callbackForm(readFileSync, [input, options, callback]);
}
export function writeFile(input, data, options, callback) {
  if (typeof options === "function") return callbackForm(writeFileSync, [input, data, options]);
  return callbackForm(writeFileSync, [input, data, options, callback]);
}

export const promises = {
  readFile: async (input, options) => readFileSync(input, options),
  writeFile: async (input, data, options) => writeFileSync(input, data, options),
  appendFile: async (input, data, options) => appendFileSync(input, data, options),
  mkdir: async (input, options) => mkdirSync(input, options),
  readdir: async (input, options) => readdirSync(input, options),
  stat: async (input) => statSync(input),
  lstat: async (input) => lstatSync(input),
  unlink: async (input) => unlinkSync(input),
  rm: async (input, options) => rmSync(input, options),
  rmdir: async (input, options) => rmdirSync(input, options),
  rename: async (from, to) => renameSync(from, to),
  copyFile: async (from, to) => copyFileSync(from, to),
  realpath: async (input) => realpathSync(input),
};

export default { readFileSync, writeFileSync, appendFileSync, mkdirSync, readdirSync, statSync, lstatSync, existsSync, unlinkSync, rmSync, rmdirSync, renameSync, copyFileSync, realpathSync, openSync, closeSync, constants, readFile, writeFile, promises };
