import { processShim } from "../globals.js";

export default processShim;
export const arch = processShim.arch;
export const argv = processShim.argv;
export const cwd = processShim.cwd;
export const env = processShim.env;
export const getBuiltinModule = (name: string): unknown => processShim.getBuiltinModule(name);
export const nextTick = (
  callback: (...arguments_: unknown[]) => void,
  ...arguments_: unknown[]
): void => {
  processShim.nextTick(callback, ...arguments_);
};
export const pid = processShim.pid;
export const platform = processShim.platform;
export const release = processShim.release;
export const umask = processShim.umask;
export const version = processShim.version;
export const versions = processShim.versions;
