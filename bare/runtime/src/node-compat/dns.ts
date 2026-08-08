import BareDns from "bare-dns";
import type { LookupOptions } from "bare-dns";

import { unsupportedMethod } from "./not-implemented.js";

export const ADDRCONFIG = 1024;
export const ADDRGETNETWORKPARAMS = "EADDRGETNETWORKPARAMS";
export const ALL = 256;
export const BADFAMILY = "EBADFAMILY";
export const BADFLAGS = "EBADFLAGS";
export const BADHINTS = "EBADHINTS";
export const BADNAME = "EBADNAME";
export const BADQUERY = "EBADQUERY";
export const BADRESP = "EBADRESP";
export const BADSTR = "EBADSTR";
export const CANCELLED = "ECANCELLED";
export const CONNREFUSED = "ECONNREFUSED";
export const DESTRUCTION = "EDESTRUCTION";
export const EOF = "EOF";
export const FILE = "EFILE";
export const FORMERR = "EFORMERR";
export const LOADIPHLPAPI = "ELOADIPHLPAPI";
export const NODATA = "ENODATA";
export const NOMEM = "ENOMEM";
export const NONAME = "ENONAME";
export const NOTFOUND = "ENOTFOUND";
export const NOTIMP = "ENOTIMP";
export const NOTINITIALIZED = "ENOTINITIALIZED";
export const REFUSED = "EREFUSED";
export const SERVFAIL = "ESERVFAIL";
export const TIMEOUT = "ETIMEOUT";
export const V4MAPPED = 2048;

export const Resolver = BareDns.Resolver;
export const lookup = BareDns.lookup;
export const resolveTxt = BareDns.resolveTxt;
export const getDefaultResultOrder = (): "verbatim" => "verbatim";
export const getServers = (): string[] => [];
export const setDefaultResultOrder = unsupportedMethod("dns", "setDefaultResultOrder");
export const setServers = unsupportedMethod("dns", "setServers");
export const lookupService = unsupportedMethod("dns", "lookupService");
export const resolve = unsupportedMethod("dns", "resolve");
export const resolve4 = unsupportedMethod("dns", "resolve4");
export const resolve6 = unsupportedMethod("dns", "resolve6");
export const resolveAny = unsupportedMethod("dns", "resolveAny");
export const resolveCaa = unsupportedMethod("dns", "resolveCaa");
export const resolveCname = unsupportedMethod("dns", "resolveCname");
export const resolveMx = unsupportedMethod("dns", "resolveMx");
export const resolveNaptr = unsupportedMethod("dns", "resolveNaptr");
export const resolveNs = unsupportedMethod("dns", "resolveNs");
export const resolvePtr = unsupportedMethod("dns", "resolvePtr");
export const resolveSoa = unsupportedMethod("dns", "resolveSoa");
export const resolveSrv = unsupportedMethod("dns", "resolveSrv");
export const reverse = unsupportedMethod("dns", "reverse");

type Address = { address: string; family: number };

function lookupPromise(hostname: string, options: LookupOptions = {}): Promise<Address | Address[]> {
  if (options.all === true) return lookupAll(hostname, options);
  return lookupOne(hostname, options);
}

function lookupAll(hostname: string, options: LookupOptions): Promise<Address[]> {
  return new Promise((resolve, reject) => {
    lookup(hostname, { ...options, all: true }, (error, addresses) => {
      if (error !== null) reject(error);
      else resolve(addresses ?? []);
    });
  });
}

function lookupOne(hostname: string, options: LookupOptions): Promise<Address> {
  return new Promise((resolve, reject) => {
    lookup(hostname, { ...options, all: false }, (error, address, family) => {
      if (error !== null) reject(error);
      else resolve({ address: address ?? "", family });
    });
  });
}

function resolveTxtPromise(hostname: string): Promise<string[][]> {
  return new Promise((resolve, reject) => {
    resolveTxt(hostname, (error: Error | null, records: string[][]) => {
      if (error !== null) reject(error);
      else resolve(records);
    });
  });
}

export class PromiseResolver {
  readonly #resolver = new BareDns.Resolver();

  resolveTxt(hostname: string): Promise<string[][]> {
    return new Promise((resolve, reject) => {
      this.#resolver.resolveTxt(hostname, (error, records) => {
        if (error !== null) reject(error);
        else resolve(records);
      });
    });
  }

  cancel(): void {
    this.#resolver.destroy();
  }
}

const dns = {
  ADDRCONFIG,
  ADDRGETNETWORKPARAMS,
  ALL,
  BADFAMILY,
  BADFLAGS,
  BADHINTS,
  BADNAME,
  BADQUERY,
  BADRESP,
  BADSTR,
  CANCELLED,
  CONNREFUSED,
  DESTRUCTION,
  EOF,
  FILE,
  FORMERR,
  LOADIPHLPAPI,
  NODATA,
  NOMEM,
  NONAME,
  NOTFOUND,
  NOTIMP,
  NOTINITIALIZED,
  REFUSED,
  Resolver,
  SERVFAIL,
  TIMEOUT,
  V4MAPPED,
  getDefaultResultOrder,
  getServers,
  lookup,
  lookupService,
  resolve,
  resolve4,
  resolve6,
  resolveAny,
  resolveCaa,
  resolveCname,
  resolveMx,
  resolveNaptr,
  resolveNs,
  resolvePtr,
  resolveSoa,
  resolveSrv,
  resolveTxt,
  reverse,
  setDefaultResultOrder,
  setServers,
};

export const promises = {
  ...dns,
  Resolver: PromiseResolver,
  lookup: lookupPromise,
  resolveTxt: resolveTxtPromise,
};

export default { ...dns, promises };
