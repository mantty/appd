const textEncoder = (value) => {
  const input = String(value);
  const bytes = [];
  for (let index = 0; index < input.length; index += 1) {
    let code = input.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff && index + 1 < input.length) {
      const low = input.charCodeAt(index + 1);
      if (low >= 0xdc00 && low <= 0xdfff) {
        code = 0x10000 + ((code - 0xd800) << 10) + low - 0xdc00;
        index += 1;
      }
    }
    if (code < 0x80) bytes.push(code);
    else if (code < 0x800) bytes.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f));
    else if (code < 0x10000) {
      bytes.push(0xe0 | (code >> 12), 0x80 | ((code >> 6) & 0x3f), 0x80 | (code & 0x3f));
    } else {
      bytes.push(0xf0 | (code >> 18), 0x80 | ((code >> 12) & 0x3f), 0x80 | ((code >> 6) & 0x3f), 0x80 | (code & 0x3f));
    }
  }
  return new Uint8Array(bytes);
};

const textDecoder = (value) => {
  const bytes = value instanceof Uint8Array ? value : new Uint8Array(value ?? []);
  let output = "";
  for (let index = 0; index < bytes.length;) {
    const first = bytes[index++];
    if (first < 0x80) {
      output += String.fromCharCode(first);
      continue;
    }
    let code;
    let width;
    if ((first & 0xe0) === 0xc0) {
      code = first & 0x1f;
      width = 1;
    } else if ((first & 0xf0) === 0xe0) {
      code = first & 0x0f;
      width = 2;
    } else {
      code = first & 0x07;
      width = 3;
    }
    for (let count = 0; count < width && index < bytes.length; count += 1) {
      code = (code << 6) | (bytes[index++] & 0x3f);
    }
    if (code <= 0xffff) output += String.fromCharCode(code);
    else {
      const adjusted = code - 0x10000;
      output += String.fromCharCode(0xd800 | (adjusted >> 10), 0xdc00 | (adjusted & 0x3ff));
    }
  }
  return output;
};

export class TextEncoder {
  encode(value = "") { return textEncoder(value); }
  encodeInto(source, destination) {
    const encoded = textEncoder(source);
    const written = Math.min(encoded.length, destination.length);
    destination.set(encoded.subarray(0, written));
    return { read: String(source).length, written };
  }
}

export class TextDecoder {
  constructor() {}
  decode(value = new Uint8Array()) { return textDecoder(value); }
}

export class Headers {
  constructor(init) {
    this.__values = new Map();
    if (init instanceof Headers) {
      for (const [name, value] of init) this.set(name, value);
    } else if (Array.isArray(init)) {
      for (const [name, value] of init) this.append(name, value);
    } else if (init) {
      for (const name of Object.keys(init)) this.append(name, init[name]);
    }
  }
  append(name, value) {
    const key = String(name).toLowerCase();
    const next = String(value);
    this.__values.set(key, this.__values.has(key) ? `${this.__values.get(key)}, ${next}` : next);
  }
  set(name, value) { this.__values.set(String(name).toLowerCase(), String(value)); }
  get(name) { return this.__values.get(String(name).toLowerCase()) ?? null; }
  has(name) { return this.__values.has(String(name).toLowerCase()); }
  delete(name) { this.__values.delete(String(name).toLowerCase()); }
  entries() { return this.__values.entries(); }
  keys() { return this.__values.keys(); }
  values() { return this.__values.values(); }
  forEach(callback, thisArg) { this.__values.forEach((value, name) => callback.call(thisArg, value, name, this)); }
  getSetCookie() { const value = this.get("set-cookie"); return value ? value.split(/, (?=[^;]+=)/) : []; }
  [Symbol.iterator]() { return this.entries(); }
}

export class URLSearchParams {
  constructor(init = "") {
    this.__values = [];
    if (init instanceof URLSearchParams) init = init.toString();
    if (typeof init === "object" && !(init instanceof String)) {
      for (const name of Object.keys(init)) this.append(name, init[name]);
    } else {
      for (const part of String(init).replace(/^\?/, "").split("&")) {
        if (!part) continue;
        const [name, value = ""] = part.split("=");
        this.append(decodeURIComponent(name.replace(/\+/g, " ")), decodeURIComponent(value.replace(/\+/g, " ")));
      }
    }
  }
  append(name, value) { this.__values.push([String(name), String(value)]); }
  set(name, value) {
    const key = String(name);
    this.delete(key);
    this.append(key, value);
  }
  get(name) { const item = this.__values.find(([key]) => key === String(name)); return item?.[1] ?? null; }
  getAll(name) { return this.__values.filter(([key]) => key === String(name)).map(([, value]) => value); }
  has(name) { return this.__values.some(([key]) => key === String(name)); }
  delete(name) { this.__values = this.__values.filter(([key]) => key !== String(name)); }
  sort() { this.__values.sort(([left], [right]) => left < right ? -1 : left > right ? 1 : 0); }
  entries() { return this.__values[Symbol.iterator](); }
  keys() { return this.__values.map(([key]) => key)[Symbol.iterator](); }
  values() { return this.__values.map(([, value]) => value)[Symbol.iterator](); }
  forEach(callback, thisArg) { for (const [key, value] of this.__values) callback.call(thisArg, value, key, this); }
  toString() { return this.__values.map(([key, value]) => `${encodeURIComponent(key)}=${encodeURIComponent(value)}`).join("&"); }
  [Symbol.iterator]() { return this.entries(); }
}

function parseUrl(input, base) {
  let value = String(input);
  if (base && !/^[a-z][a-z\d+.-]*:/i.test(value)) {
    const parent = new URL(base);
    if (value.startsWith("/")) value = `${parent.origin}${value}`;
    else value = `${parent.origin}${parent.pathname.slice(0, parent.pathname.lastIndexOf("/") + 1)}${value}`;
  }
  const match = value.match(/^([a-z][a-z\d+.-]*:)?(?:\/\/([^/?#]*))?([^?#]*)(?:\?([^#]*))?(?:#(.*))?$/i);
  if (!match) throw new TypeError(`Invalid URL: ${value}`);
  const protocol = (match[1] ?? "http:").toLowerCase();
  const authority = match[2] ?? "";
  const path = match[3] || "/";
  const [usernamePassword, hostPort] = authority.includes("@") ? authority.split(/@(.+)/) : ["", authority];
  const [username = "", password = ""] = usernamePassword.includes(":") ? usernamePassword.split(/:(.*)/) : [usernamePassword, ""];
  const bracketed = hostPort.startsWith("[");
  const portIndex = bracketed ? hostPort.lastIndexOf("]:") : hostPort.lastIndexOf(":");
  const hostname = portIndex > -1 ? hostPort.slice(0, portIndex + (bracketed ? 1 : 0)) : hostPort;
  const port = portIndex > -1 ? hostPort.slice(portIndex + (bracketed ? 2 : 1)) : "";
  const host = port ? `${hostname}:${port}` : hostname;
  return { protocol, username, password, hostname, port, host, pathname: path.startsWith("/") ? path : `/${path}`, search: match[4] ? `?${match[4]}` : "", hash: match[5] ? `#${match[5]}` : "" };
}

export class URL {
  constructor(input, base) { this.__set(parseUrl(input, base)); }
  __set(parts) {
    Object.assign(this, parts);
    this.origin = this.protocol === "file:" ? "null" : `${this.protocol}//${this.host}`;
    this.href = `${this.protocol}//${this.host}${this.pathname}${this.search}${this.hash}`;
    this.searchParams = new URLSearchParams(this.search);
  }
  toString() { return this.href; }
  toJSON() { return this.href; }
  static canParse(input, base) { try { new URL(input, base); return true; } catch { return false; } }
  static parse(input, base) { try { return new URL(input, base); } catch { return null; } }
}

export class Request {
  constructor(input, init = {}) {
    if (input instanceof Request) {
      this.url = input.url;
      this.method = input.method;
      this.headers = new Headers(input.headers);
      this.__body = input.__body;
    } else {
      this.url = String(input);
      this.method = String(init.method ?? "GET").toUpperCase();
      this.headers = new Headers(init.headers);
      this.__body = init.body == null ? null : init.body instanceof Uint8Array ? init.body : String(init.body);
    }
    this.bodyUsed = false;
    this.redirect = init.redirect ?? "follow";
    this.signal = init.signal ?? { aborted: false };
    this.cf = init.cf;
  }
  get body() { return this.__body == null ? null : { __appd_body: this.__body }; }
  async text() { this.bodyUsed = true; return this.__body instanceof Uint8Array ? new TextDecoder().decode(this.__body) : this.__body ?? ""; }
  async json() { return JSON.parse(await this.text()); }
  async arrayBuffer() { return (this.__body instanceof Uint8Array ? this.__body : new TextEncoder().encode(this.__body ?? "")).buffer; }
  clone() { return new Request(this); }
}

export class Response {
  constructor(body = null, init = {}) {
    if (body instanceof Response) {
      init = { ...body, headers: new Headers(body.headers) };
      body = body.__stream ?? body.__body;
    }
    this.status = init.status ?? 200;
    this.statusText = init.statusText ?? "";
    this.headers = new Headers(init.headers);
    this.__stream = body instanceof ReadableStream ? body : null;
    this.__body = this.__stream ? null : body == null ? null : body instanceof Uint8Array ? body : String(body);
    this.__appd_body = this.__body;
    this.body = this.__stream ?? (this.__body == null ? null : { __appd_body: this.__body });
    this.ok = this.status >= 200 && this.status < 300;
    this.redirected = false;
    this.type = "default";
    this.url = "";
    this.webSocket = init.webSocket;
  }
  async text() {
    if (!this.__stream) return this.__body instanceof Uint8Array ? new TextDecoder().decode(this.__body) : this.__body ?? "";
    const reader = this.__stream.getReader();
    const chunks = [];
    while (true) {
      const result = await reader.read();
      if (result.done) break;
      chunks.push(result.value instanceof Uint8Array ? new TextDecoder().decode(result.value) : String(result.value));
    }
    reader.releaseLock();
    return chunks.join("");
  }
  async json() { return JSON.parse(await this.text()); }
  async arrayBuffer() { return new TextEncoder().encode(await this.text()).buffer; }
  clone() { return new Response(this.__body, { status: this.status, statusText: this.statusText, headers: this.headers }); }
  static error() { return new Response(null, { status: 0 }); }
  static redirect(url, status = 302) { return new Response(null, { status, headers: { location: url } }); }
}

export class ReadableStream {
  constructor(source = {}) {
    this.locked = false;
    this.__queue = [];
    this.__readers = [];
    this.__closed = false;
    const controller = {
      enqueue: (value) => {
        if (this.__closed) return;
        const reader = this.__readers.shift();
        if (reader) reader({ done: false, value });
        else this.__queue.push(value);
      },
      close: () => {
        this.__closed = true;
        while (this.__readers.length) this.__readers.shift()({ done: true, value: undefined });
      },
      error: (error) => {
        this.__closed = true;
        while (this.__readers.length) this.__readers.shift()(Promise.reject(error));
      },
    };
    try {
      const started = source.start?.(controller);
      if (started?.catch) started.catch(controller.error);
    } catch (error) {
      controller.error(error);
    }
  }
  getReader() {
    this.locked = true;
    return {
      read: () => {
        if (this.__queue.length) return Promise.resolve({ done: false, value: this.__queue.shift() });
        if (this.__closed) return Promise.resolve({ done: true, value: undefined });
        return new Promise((resolve) => this.__readers.push(resolve));
      },
      releaseLock: () => { this.locked = false; },
    };
  }
  cancel() { this.__closed = true; return Promise.resolve(); }
  tee() { return [new ReadableStream(), new ReadableStream()]; }
}

export class Blob {
  constructor(parts = [], options = {}) {
    const chunks = parts.map((part) => {
      if (part instanceof Uint8Array) return new Uint8Array(part);
      if (part instanceof ArrayBuffer) return new Uint8Array(part);
      if (ArrayBuffer.isView(part)) return new Uint8Array(part.buffer, part.byteOffset, part.byteLength);
      return new TextEncoder().encode(String(part));
    });
    const length = chunks.reduce((total, chunk) => total + chunk.length, 0);
    this.__bytes = new Uint8Array(length);
    let offset = 0;
    for (const chunk of chunks) {
      this.__bytes.set(chunk, offset);
      offset += chunk.length;
    }
    this.type = options.type ?? "";
    this.size = this.__bytes.length;
  }
  async text() { return new TextDecoder().decode(this.__bytes); }
  async arrayBuffer() { return this.__bytes.slice().buffer; }
}

export class DOMException extends Error {
  constructor(message = "", name = "Error") { super(message); this.name = name; }
}

class DateTimeFormat {
  constructor(locales = [], options = {}) {
    this.locales = locales;
    this.options = options;
  }
  format(value = new Date()) {
    const date = value instanceof Date ? value : new Date(value);
    const options = this.options;
    const pad = (part) => String(part).padStart(2, "0");
    const time = `${pad(date.getUTCHours())}:${pad(date.getUTCMinutes())}:${pad(date.getUTCSeconds())}`;
    if (options.hour || options.minute || options.second) return time;
    return `${pad(date.getUTCDate())}/${pad(date.getUTCMonth() + 1)}/${date.getUTCFullYear()}, ${time}`;
  }
  resolvedOptions() { return { locale: "en-GB", timeZone: "UTC", ...this.options }; }
  formatToParts(value = new Date()) { return [{ type: "literal", value: this.format(value) }]; }
}

class NumberFormat {
  constructor(locales = [], options = {}) { this.locales = locales; this.options = options; }
  format(value) { return String(value); }
  formatToParts(value) { return [{ type: "integer", value: this.format(value) }]; }
  resolvedOptions() { return { locale: "en-GB", ...this.options }; }
}

class PluralRules {
  select(value) { return Number(value) === 1 ? "one" : "other"; }
  resolvedOptions() { return { locale: "en-GB", type: "cardinal" }; }
}

const intl = { DateTimeFormat, NumberFormat, PluralRules, getCanonicalLocales: (locales) => Array.isArray(locales) ? locales : [locales] };

export function installWebGlobals() {
  const globals = { TextEncoder, TextDecoder, Headers, URL, URLSearchParams, Request, Response, ReadableStream, Blob, DOMException };
  for (const [name, value] of Object.entries(globals)) globalThis[name] = value;
  globalThis.Intl ??= intl;
  if (!Date.prototype.toLocaleString) Date.prototype.toLocaleString = function toLocaleString(locales, options) { return new DateTimeFormat(locales, options).format(this); };
  globalThis.WebAssembly ??= {
    compile: async () => ({}),
    instantiate: async () => ({ exports: {} }),
  };
  globalThis.atob ??= (value) => new TextDecoder().decode(new Uint8Array(Array.from(String(value), (char) => char.charCodeAt(0))));
  globalThis.btoa ??= (value) => String(value);
  globalThis.crypto ??= {};
  globalThis.crypto.getRandomValues ??= (array) => { for (let index = 0; index < array.length; index += 1) array[index] = Math.floor(Math.random() * 2 ** 32); return array; };
  globalThis.performance ??= { timeOrigin: Date.now(), now: () => Date.now() };
}
