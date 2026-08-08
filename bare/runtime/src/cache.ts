import crypto from "bare-crypto";
import fs from "bare-fs";
import path from "bare-path";
import Stream from "bare-stream";

import { RuntimeResponse } from "./response.js";

type CacheRequest = Request | string;
type HeaderEntry = readonly [string, string];
type VaryEntry = readonly [string, string | null];

interface CacheEntry {
  readonly expiresAt: number;
  readonly headers: readonly HeaderEntry[];
  readonly key: string;
  readonly status: number;
  readonly statusText: string;
  readonly vary?: readonly VaryEntry[];
}

interface CacheMetadata {
  readonly name: string;
}

interface MatchOptions {
  readonly ignoreMethod?: boolean;
}

export class CacheStorage {
  readonly #caches = new Map<string, Cache>();
  #root: string | undefined;

  get default(): Cache {
    return this.cache("default");
  }

  configure(root: string): void {
    if (root === "") throw new TypeError("Cache storage requires a directory");
    this.#root = root;
    this.#caches.clear();
  }

  async delete(name: string): Promise<boolean> {
    const directory = this.directory(name);
    if (!exists(directory)) return false;
    fs.rmSync(directory, { force: true, recursive: true });
    this.#caches.delete(name);
    return true;
  }

  async has(name: string): Promise<boolean> {
    return this.cacheName(this.directory(name)).includes(name);
  }

  async keys(): Promise<string[]> {
    const root = this.root();
    if (!exists(root)) return [];
    return fs.readdirSync(root)
      .filter((entry) => entry.startsWith("cache-"))
      .flatMap((entry) => this.cacheName(path.join(root, entry)))
      .sort();
  }

  async match(request: CacheRequest, options?: MatchOptions): Promise<Response | undefined> {
    return this.default.match(request, options);
  }

  async open(name: string): Promise<Cache> {
    this.ensure(name);
    return this.cache(name);
  }

  directory(name: string): string {
    this.validateName(name);
    return path.join(this.root(), `cache-${digest(name)}`);
  }

  ensure(name: string): void {
    const directory = this.directory(name);
    fs.mkdirSync(directory, { recursive: true });
    const metadataPath = path.join(directory, "cache.json");
    const metadata = readJson<CacheMetadata>(metadataPath);
    if (metadata === undefined) {
      writeJson(metadataPath, { name });
      return;
    }
    if (metadata.name !== name) throw new Error("Cache storage name collision");
  }

  private cache(name: string): Cache {
    this.validateName(name);
    let cache = this.#caches.get(name);
    if (cache === undefined) {
      cache = new Cache(this, name);
      this.#caches.set(name, cache);
    }
    return cache;
  }

  private cacheName(directory: string): string[] {
    const metadata = readJson<CacheMetadata>(path.join(directory, "cache.json"));
    return metadata === undefined ? [] : [metadata.name];
  }

  private root(): string {
    if (this.#root === undefined) throw new Error("Cache storage is not configured");
    return this.#root;
  }

  private validateName(name: string): void {
    if (typeof name !== "string" || name === "") throw new TypeError("Cache name must be a non-empty string");
  }
}

export class Cache {
  readonly #storage: CacheStorage;
  readonly #name: string;
  #tail: Promise<void> = Promise.resolve();

  constructor(storage: CacheStorage, name: string) {
    this.#storage = storage;
    this.#name = name;
  }

  async delete(request: CacheRequest, options?: MatchOptions): Promise<boolean> {
    return this.queue(() => this.remove(request, options));
  }

  async keys(request?: CacheRequest, options?: MatchOptions): Promise<Request[]> {
    return this.queue(() => this.list(request, options));
  }

  async match(request: CacheRequest, options?: MatchOptions): Promise<Response | undefined> {
    return this.queue(() => this.find(request, options));
  }

  async put(request: CacheRequest, response: Response): Promise<void> {
    return this.queue(() => this.store(request, response));
  }

  private async find(input: CacheRequest, options?: MatchOptions): Promise<Response | undefined> {
    const request = cacheRequest(input);
    if (request.method !== "GET" && options?.ignoreMethod !== true) return undefined;
    const match = this.matchingEntry(request);
    if (match === undefined) return undefined;
    const { entry, paths } = match;
    if (notModified(request, entry.headers)) return notModifiedResponse(entry);
    const range = rangeResponse(request, entry.headers);
    return responseFrom(entry, paths.body, range);
  }

  private async list(input?: CacheRequest, options?: MatchOptions): Promise<Request[]> {
    if (input !== undefined) {
      const request = cacheRequest(input);
      if (request.method !== "GET" && options?.ignoreMethod !== true) return [];
      const match = this.matchingEntry(request);
      return match === undefined ? [] : [requestFromEntry(match.entry)];
    }
    return this.entryPaths()
      .flatMap((pathname) => this.entryRequest(pathname))
      .sort((left, right) => left.url.localeCompare(right.url));
  }

  private async remove(input: CacheRequest, options?: MatchOptions): Promise<boolean> {
    const request = cacheRequest(input);
    if (request.method !== "GET" && options?.ignoreMethod !== true) return false;
    const match = this.matchingEntry(request);
    if (match === undefined) return false;
    remove(match.paths);
    return true;
  }

  private async store(input: CacheRequest, response: Response): Promise<void> {
    const request = cacheRequest(input);
    validatePut(request, response);
    const expiresAt = expiration(response);
    if (expiresAt <= Date.now()) return;
    const stored = entry(request, response, expiresAt);
    const paths = this.paths(stored.key, stored.vary);
    this.#storage.ensure(this.#name);
    const temporary = `${paths.body}.${crypto.randomUUID()}.tmp`;
    try {
      await writeBody(response.clone(), temporary);
      fs.renameSync(temporary, paths.body);
      writeJson(paths.entry, stored);
      this.removeDifferentVariants(stored);
    } catch (error) {
      fs.rmSync(temporary, { force: true });
      throw error;
    }
  }

  private directory(): string {
    return this.#storage.directory(this.#name);
  }

  private entryRequest(pathname: string): Request[] {
    const entry = readJson<CacheEntry>(pathname);
    if (entry === undefined || entry.expiresAt <= Date.now()) return [];
    return [requestFromEntry(entry)];
  }

  private matchingEntry(request: Request): CachedEntry | undefined {
    for (const pathname of this.entryPaths()) {
      const entry = readJson<CacheEntry>(pathname);
      if (entry === undefined || entry.key !== request.url || !varies(request, entry)) continue;
      const paths = pathsForEntry(pathname);
      if (entry.expiresAt <= Date.now()) {
        remove(paths);
        continue;
      }
      return { entry, paths };
    }
    return undefined;
  }

  private removeDifferentVariants(next: CacheEntry): void {
    for (const pathname of this.entryPaths()) {
      const entry = readJson<CacheEntry>(pathname);
      if (entry !== undefined && entry.key === next.key && !sameVary(entry, next)) {
        remove(pathsForEntry(pathname));
      }
    }
  }

  private entryPaths(): string[] {
    const directory = this.directory();
    if (!exists(directory)) return [];
    return fs.readdirSync(directory)
      .filter((entry) => entry.startsWith("entry-") && entry.endsWith(".json"))
      .map((entry) => path.join(directory, entry))
      .sort();
  }

  private paths(key: string, vary: readonly VaryEntry[] | undefined): CachePaths {
    const identity = vary === undefined || vary.length === 0 ? key : JSON.stringify([key, vary]);
    const prefix = path.join(this.directory(), `entry-${digest(identity)}`);
    return { body: `${prefix}.body`, entry: `${prefix}.json` };
  }

  private async queue<T>(operation: () => Promise<T>): Promise<T> {
    const result = this.#tail.then(operation, operation);
    this.#tail = result.then(voidResult, voidResult);
    return result;
  }
}

interface CachePaths {
  readonly body: string;
  readonly entry: string;
}

interface CachedEntry {
  readonly entry: CacheEntry;
  readonly paths: CachePaths;
}

interface ByteRange {
  readonly end: number;
  readonly start: number;
  readonly total: number;
}

export const caches = new CacheStorage();

export function configureCaches(root: string): void {
  caches.configure(root);
}

function cacheRequest(input: CacheRequest): Request {
  return typeof input === "string" ? new Request(input) : input;
}

function digest(value: string): string {
  return crypto.createHash("sha-256").update(value).digest("hex");
}

function entry(request: Request, response: Response, expiresAt: number): CacheEntry {
  return {
    expiresAt,
    headers: headers(response.headers),
    key: request.url,
    status: response.status,
    statusText: response.statusText,
    vary: vary(response, request),
  };
}

function exists(pathname: string): boolean {
  try {
    fs.accessSync(pathname);
    return true;
  } catch (error) {
    if (isMissing(error)) return false;
    throw error;
  }
}

function expiration(response: Response): number {
  const control = response.headers.get("cache-control");
  const seconds = cacheSeconds(control);
  if (seconds !== undefined) return Date.now() + seconds * 1_000;
  const expires = response.headers.get("expires");
  if (expires !== null) {
    const timestamp = Date.parse(expires);
    if (!Number.isNaN(timestamp)) return timestamp;
  }
  return Date.now() + defaultSeconds(response.status) * 1_000;
}

function cacheSeconds(value: string | null): number | undefined {
  if (value === null) return undefined;
  const directives = new Map<string, string | undefined>();
  for (const part of value.split(",")) {
    const [name = "", parameter] = part.trim().split("=", 2);
    directives.set(name.toLowerCase(), parameter?.replace(/^"|"$/g, ""));
  }
  if (forbidsCaching(directives)) throw new TypeError("Cache-Control forbids caching this response");
  const value_ = directives.get("s-maxage") ?? directives.get("max-age");
  if (value_ === undefined) return undefined;
  const seconds = Number(value_);
  if (!Number.isInteger(seconds) || seconds < 0) throw new TypeError("Cache-Control has an invalid max-age");
  if (seconds === 0) throw new TypeError("Cache-Control forbids caching this response");
  return seconds;
}

function forbidsCaching(directives: ReadonlyMap<string, string | undefined>): boolean {
  if (directives.has("no-cache") || directives.has("no-store")) return true;
  const private_ = directives.get("private");
  return directives.has("private") && private_?.toLowerCase() !== "set-cookie";
}

function defaultSeconds(status: number): number {
  if (status === 200 || status === 301) return 7_200;
  if (status === 302 || status === 303) return 1_200;
  if (status === 404 || status === 410) return 180;
  return 0;
}

function headers(source: Headers): HeaderEntry[] {
  const entries = [...source].filter(([name]) => name !== "set-cookie") as HeaderEntry[];
  const cookies = (source as Headers & { getSetCookie?: () => string[] }).getSetCookie?.() ?? [];
  return [...entries, ...cookies.map((cookie) => ["set-cookie", cookie] as const)];
}

function vary(response: Response, request: Request): VaryEntry[] {
  return varyNames(response.headers).map((name) => [name, request.headers.get(name)] as const);
}

function varyNames(headers: Headers): string[] {
  const value = headers.get("vary");
  if (value === null) return [];
  return [...new Set(value.split(",").map((name) => name.trim().toLowerCase()).filter(Boolean))].sort();
}

function varies(request: Request, entry: CacheEntry): boolean {
  return entry.vary?.every(([name, value]) => request.headers.get(name) === value) ?? true;
}

function sameVary(left: CacheEntry, right: CacheEntry): boolean {
  const leftNames = (left.vary ?? []).map(([name]) => name);
  const rightNames = (right.vary ?? []).map(([name]) => name);
  return JSON.stringify(leftNames) === JSON.stringify(rightNames);
}

function pathsForEntry(entry: string): CachePaths {
  const prefix = entry.slice(0, -".json".length);
  return { body: `${prefix}.body`, entry };
}

function requestFromEntry(entry: CacheEntry): Request {
  const headers = Object.fromEntries(
    (entry.vary ?? []).flatMap(([name, value]) => value === null ? [] : [[name, value]]),
  );
  return new Request(entry.key, { headers });
}

function notModified(request: Request, headers: readonly HeaderEntry[]): boolean {
  const etag = header(headers, "etag");
  const noneMatch = request.headers.get("if-none-match");
  if (etag !== null && noneMatch !== null && etagMatches(noneMatch, etag)) return true;
  const lastModified = header(headers, "last-modified");
  const modifiedSince = request.headers.get("if-modified-since");
  return lastModified !== null
    && modifiedSince !== null
    && Date.parse(lastModified) <= Date.parse(modifiedSince);
}

function etagMatches(values: string, etag: string): boolean {
  return values.split(",").some((value) => value.trim() === "*" || weakEtag(value) === weakEtag(etag));
}

function weakEtag(value: string): string {
  return value.trim().replace(/^W\//i, "");
}

function header(entries: readonly HeaderEntry[], name: string): string | null {
  return entries.find(([entry]) => entry === name)?.[1] ?? null;
}

function rangeResponse(request: Request, headers: readonly HeaderEntry[]): ByteRange | null {
  const range = request.headers.get("range");
  const length = Number(header(headers, "content-length"));
  if (range === null || !Number.isSafeInteger(length) || length < 0) return null;
  const match = /^bytes=(\d*)-(\d*)$/.exec(range);
  if (match === null) return null;
  const start = rangeStart(match[1] ?? "", match[2] ?? "", length);
  const end = rangeEnd(match[1] ?? "", match[2] ?? "", length);
  if (start === undefined || end === undefined || start > end) return null;
  return { end, start, total: length };
}

function rangeStart(start: string, end: string, total: number): number | undefined {
  if (start !== "") return Number(start);
  if (end === "") return undefined;
  const suffix = Number(end);
  return suffix > total ? 0 : total - suffix;
}

function rangeEnd(start: string, end: string, total: number): number | undefined {
  if (end !== "") return Math.min(Number(end), total - 1);
  if (start !== "") return total - 1;
  return undefined;
}

function remove(paths: CachePaths): void {
  fs.rmSync(paths.entry, { force: true });
  fs.rmSync(paths.body, { force: true });
}

function responseFrom(entry: CacheEntry, body: string, range: ByteRange | null): Response {
  const headers = new Headers(entry.headers.map(([name, value]) => [name, value] as [string, string]));
  if (range === null) return new RuntimeResponse(bodyStream(body), responseInit(entry, headers));
  headers.set("content-length", String(range.end - range.start + 1));
  headers.set("content-range", `bytes ${range.start}-${range.end}/${range.total}`);
  return new RuntimeResponse(bodyStream(body, range), {
    headers,
    status: 206,
    statusText: "Partial Content",
  });
}

function notModifiedResponse(entry: CacheEntry): Response {
  return new RuntimeResponse(null, {
    headers: new Headers(entry.headers.map(([name, value]) => [name, value] as [string, string])),
    status: 304,
    statusText: "Not Modified",
  });
}

function responseInit(entry: CacheEntry, headers: Headers): ResponseInit {
  return { headers, status: entry.status, statusText: entry.statusText };
}

function bodyStream(pathname: string, range?: ByteRange): ReadableStream<Uint8Array> {
  const options = {
    ...(range === undefined ? {} : { end: range.end, start: range.start }),
    eagerOpen: false,
  };
  return Stream.Readable.toWeb(fs.createReadStream(pathname, options as never)) as unknown as ReadableStream<Uint8Array>;
}

function readJson<T>(pathname: string): T | undefined {
  try {
    return JSON.parse(readFile(pathname)) as T;
  } catch (error) {
    if (isMissing(error)) return undefined;
    if (!(error instanceof SyntaxError)) throw error;
    fs.rmSync(pathname, { force: true });
    return undefined;
  }
}

function readFile(pathname: string): string {
  return fs.readFileSync(pathname, "utf8") as string;
}

function writeJson(pathname: string, value: unknown): void {
  const temporary = `${pathname}.${crypto.randomUUID()}.tmp`;
  fs.writeFileSync(temporary, JSON.stringify(value));
  fs.renameSync(temporary, pathname);
}

async function writeBody(response: Response, pathname: string): Promise<void> {
  const body = response.body;
  if (body === null) {
    fs.writeFileSync(pathname, new Uint8Array(0));
    return;
  }
  await new Promise<void>((resolve, reject) => {
    Stream.pipeline(Stream.Readable.fromWeb(body as unknown as Parameters<typeof Stream.Readable.fromWeb>[0]), fs.createWriteStream(pathname), (error) => {
      if (error === null) resolve();
      else reject(error);
    });
  });
}

function validatePut(request: Request, response: Response): void {
  if (request.method !== "GET") throw new TypeError("Cache only accepts GET requests");
  if (response.status === 206) throw new TypeError("Cache does not accept partial responses");
  if (varyNames(response.headers).includes("*")) throw new TypeError("Cache does not accept Vary: * responses");
  if (response.headers.getSetCookie().length > 0 && !allowsSetCookie(response.headers.get("cache-control"))) {
    throw new TypeError("Cache does not accept Set-Cookie responses");
  }
}

function allowsSetCookie(cacheControl: string | null): boolean {
  return cacheControl?.split(",").some((value) => /^private\s*=\s*"?set-cookie"?$/i.test(value.trim())) ?? false;
}

function isMissing(error: unknown): boolean {
  return typeof error === "object" && error !== null && "code" in error && error.code === "ENOENT";
}

function voidResult(): void {}
