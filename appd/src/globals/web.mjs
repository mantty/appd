import { Blob, Headers, Request, Response } from "../network/fetch.mjs";
import { URL, URLSearchParams } from "../network/url.mjs";
import { ReadableStream, TransformStream, WritableStream } from "../streams/web.mjs";
import { TextDecoder, TextEncoder } from "../streams/text.mjs";

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
  const globals = { TextEncoder, TextDecoder, Headers, URL, URLSearchParams, Request, Response, ReadableStream, WritableStream, TransformStream, Blob, DOMException };
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
