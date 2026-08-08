import { PromiseResolver, promises } from "./dns.js";

export * from "./dns.js";
export const lookup = promises.lookup;
export const resolveTxt = promises.resolveTxt;
export const Resolver = PromiseResolver;
export default promises;
