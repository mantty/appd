import Buffer from "bare-buffer";
import inspect from "bare-inspect";
import { parse as parseMIME } from "bare-mime";

import types from "./util-types.js";
import { isDeepStrictEqual } from "./deep-equal.js";

type Callback = (error: Error | null, value?: unknown) => void;
type DebugLogger = ((message: string, ...arguments_: unknown[]) => void) & { enabled: boolean };
type Promisified = (...arguments_: unknown[]) => Promise<unknown>;

export const isArray = Array.isArray;
export const isBoolean = (value: unknown): boolean => typeof value === "boolean";
export const isBuffer = Buffer.isBuffer;
export const isDate = (value: unknown): boolean => value instanceof Date;
export const isError = (value: unknown): boolean => value instanceof Error;
export const isFunction = (value: unknown): boolean => typeof value === "function";
export const isNull = (value: unknown): boolean => value === null;
export const isNullOrUndefined = (value: unknown): boolean => value === null || value === undefined;
export const isNumber = (value: unknown): boolean => typeof value === "number";
export const isObject = (value: unknown): boolean => value !== null && (typeof value === "object" || typeof value === "function");
export const isPrimitive = (value: unknown): boolean => !isObject(value);
export const isRegExp = (value: unknown): boolean => value instanceof RegExp;
export const isString = (value: unknown): boolean => typeof value === "string";
export const isSymbol = (value: unknown): boolean => typeof value === "symbol";
export const isUndefined = (value: unknown): boolean => value === undefined;

export function inherits(child: Function, parent: Function): void {
  (child as Function & { super_?: Function }).super_ = parent;
  child.prototype = Object.create(parent.prototype, { constructor: { configurable: true, value: child, writable: true } });
}

export function format(template: unknown, ...arguments_: unknown[]): string {
  if (typeof template !== "string") return [template, ...arguments_].map((value) => inspect(value)).join(" ");

  let index = 0;
  const formatted = template.replace(/%[sdijoO%]/g, (specifier) => {
    if (specifier === "%%") return "%";
    if (index === arguments_.length) return specifier;
    const value = arguments_[index++];
    if (specifier === "%s") return String(value);
    if (specifier === "%d") return String(Number(value));
    if (specifier === "%i") return String(Number.parseInt(String(value), 10));
    if (specifier === "%j") {
      try {
        return JSON.stringify(value);
      } catch {
        return "[Circular]";
      }
    }
    return inspect(value);
  });

  return index === arguments_.length
    ? formatted
    : `${formatted} ${arguments_.slice(index).map((value) => inspect(value)).join(" ")}`;
}

export function formatWithOptions(_options: unknown, template: unknown, ...arguments_: unknown[]): string {
  return format(template, ...arguments_);
}

export function log(...arguments_: unknown[]): void {
  console.log(...arguments_);
}

export function debuglog(_section: string, callback?: (logger: DebugLogger) => void): DebugLogger {
  const logger = Object.assign((): void => {}, { enabled: false }) as DebugLogger;
  callback?.(logger);
  return logger;
}

export const debug = debuglog;

const promisifyCustom = Symbol.for("nodejs.util.promisify.custom");

function promisifyFunction(function_: Function): Promisified {
  const custom = (function_ as Function & { [promisifyCustom]?: unknown })[promisifyCustom];
  if (custom !== undefined) {
    if (typeof custom !== "function") throw new TypeError("The custom promisify property must be a function");
    return custom as Promisified;
  }

  return function promisified(this: unknown, ...arguments_: unknown[]): Promise<unknown> {
    return new Promise((resolve, reject) => {
      const callback: Callback = (error, value) => { error === null ? resolve(value) : reject(error); };
      Reflect.apply(function_, this, [...arguments_, callback]);
    });
  };
}

export const promisify = Object.assign(promisifyFunction, { custom: promisifyCustom });

export class MIMEParams implements Iterable<[string, string]> {
  readonly #values = new Map<string, string>();

  get size(): number {
    return this.#values.size;
  }

  get(name: string): string | null {
    return this.#values.get(name) ?? null;
  }

  set(name: string, value: string): void {
    this.#values.set(validateParameterName(name), validateParameterValue(value));
  }

  has(name: string): boolean {
    return this.#values.has(name);
  }

  delete(name: string): void {
    this.#values.delete(name);
  }

  entries(): IterableIterator<[string, string]> {
    return this.#values.entries();
  }

  [Symbol.iterator](): IterableIterator<[string, string]> {
    return this.entries();
  }

  toString(): string {
    return [...this.#values].map(([name, value]) => `${name}=${serializeParameterValue(value)}`).join(";");
  }
}

export class MIMEType {
  readonly params = new MIMEParams();
  #type: string;
  #subtype: string;

  constructor(value: string) {
    const parsed = parseMIME(value);
    if (parsed === null) throw new TypeError("Invalid MIME type");
    this.#type = parsed.type;
    this.#subtype = parsed.subtype;
    for (const [name, parameter] of parsed.parameters) this.params.set(name.toLowerCase(), parameter);
  }

  get type(): string {
    return this.#type;
  }

  set type(value: string) {
    this.#type = normalizeToken(value);
  }

  get subtype(): string {
    return this.#subtype;
  }

  set subtype(value: string) {
    this.#subtype = normalizeToken(value);
  }

  get essence(): string {
    return `${this.#type}/${this.#subtype}`;
  }

  toString(): string {
    const parameters = this.params.toString();
    return parameters === "" ? this.essence : `${this.essence};${parameters}`;
  }
}

function validateParameterName(value: string): string {
  if (!isToken(value)) throw new TypeError("Invalid MIME token");
  return value;
}

function normalizeToken(value: string): string {
  if (!isToken(value)) throw new TypeError("Invalid MIME token");
  return value.toLowerCase();
}

function validateParameterValue(value: string): string {
  if (!/^[\t\x20-\x7e\x80-\xff]*$/.test(value)) throw new TypeError("Invalid MIME parameter value");
  return value;
}

function serializeParameterValue(value: string): string {
  return isToken(value) ? value : `"${value.replace(/[\\"]/g, "\\$&")}"`;
}

function isToken(value: string): boolean {
  return /^[!#$%&'*+\-.^_`|~A-Za-z0-9]+$/.test(value);
}

export const TextDecoder = globalThis.TextDecoder;
export const TextEncoder = globalThis.TextEncoder;
export const _extend = Object.assign;
export const deprecate = <Function_ extends (...arguments_: unknown[]) => unknown>(function_: Function_): Function_ => function_;

export function callbackify<Function_ extends (...arguments_: unknown[]) => Promise<unknown>>(
  function_: Function_,
): (...arguments_: unknown[]) => void {
  return function callbackified(this: unknown, ...arguments_: unknown[]): void {
    const callback = arguments_.pop();
    if (typeof callback !== "function") throw new TypeError("The last argument must be a function");
    void Reflect.apply(function_, this, arguments_).then(
      (value) => { callback(null, value); },
      (error: unknown) => { callback(error instanceof Error ? error : new Error(String(error))); },
    );
  };
}

const util = {
  _extend,
  callbackify,
  debug,
  debuglog,
  deprecate,
  format,
  formatWithOptions,
  inherits,
  inspect,
  isArray,
  isBoolean,
  isBuffer,
  isDate,
  isDeepStrictEqual,
  isError,
  isFunction,
  isNull,
  isNullOrUndefined,
  isNumber,
  isObject,
  isPrimitive,
  isRegExp,
  isString,
  isSymbol,
  isUndefined,
  log,
  MIMEParams,
  MIMEType,
  promisify,
  TextDecoder,
  TextEncoder,
  types,
};

export default util;
