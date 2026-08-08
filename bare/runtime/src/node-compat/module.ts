import { builtin, builtinNames } from "./registry.js";

interface RuntimeProcess {
  getBuiltinModule(name: string): unknown;
}

interface Require {
  (name: string): unknown;
  readonly cache: Record<string, never>;
  readonly extensions: Record<string, never>;
  readonly main: undefined;
}

export const builtinModules = Object.freeze([...builtinNames()]);

export function isBuiltin(name: string): boolean {
  return builtin(name) !== undefined;
}

export function createRequire(_filename: string | URL): Require {
  const require = ((name: string): unknown => {
    const module = runtimeProcess().getBuiltinModule(name);
    if (module !== undefined) return module;
    throw new Error(`Cannot find module '${name}'`);
  }) as Require;

  Object.defineProperties(require, {
    cache: { value: Object.create(null) },
    extensions: { value: Object.create(null) },
    main: { value: undefined },
  });
  return require;
}

export const Module = Object.assign(
  function Module(): never {
    throw new Error("The module.Module constructor is not implemented");
  },
  { builtinModules, createRequire, isBuiltin },
);

function runtimeProcess(): RuntimeProcess {
  const process = (globalThis as unknown as { process?: RuntimeProcess }).process;
  if (process === undefined) throw new Error("The appd process runtime is not initialized");
  return process;
}

export default Module;
