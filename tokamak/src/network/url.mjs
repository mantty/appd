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
