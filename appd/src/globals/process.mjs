export function installProcessGlobals(builtinModules) {
  const nativeGetBuiltinModule = globalThis.process?.getBuiltinModule;
  globalThis.process ??= {};
  globalThis.process.env ??= {};
  globalThis.process.nextTick ??= (callback, ...args) => queueMicrotask(() => callback(...args));
  globalThis.process.getBuiltinModule = (name) => builtinModules[name] ?? nativeGetBuiltinModule?.(name);
  globalThis.process.platform ??= "appd";
  globalThis.process.arch ??= "unknown";
  globalThis.process.versions ??= { node: "22.14.0" };
  globalThis.process.cwd ??= () => "/bundle";
  globalThis.process.hrtime ??= (start) => {
    const now = Date.now();
    const value = [Math.floor(now / 1000), (now % 1000) * 1e6];
    if (!start) return value;
    const seconds = value[0] - start[0];
    const nanos = value[1] - start[1];
    return nanos < 0 ? [seconds - 1, nanos + 1e9] : [seconds, nanos];
  };
}
