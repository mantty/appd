import { errno, priority, signals } from "./constants.js";

export const EOL = "\n";
export const devNull = "/dev/null";
export const constants = Object.freeze({ UV_UDP_REUSEADDR: 4, dlopen: Object.freeze({}), errno, priority, signals });
export const arch = (): string => "x64";
export const availableParallelism = (): number => 1;
export const cpus = (): never[] => [];
export const endianness = (): string => "LE";
export const freemem = (): number => 0;
export const getPriority = (): number => 0;
export const homedir = (): string => "/tmp/";
export const hostname = (): string => "localhost";
export const loadavg = (): [number, number, number] => [0, 0, 0];
export const machine = (): string => "x86_64";
export const networkInterfaces = (): Record<string, never> => ({});
export const platform = (): string => "linux";
export const release = (): string => "";
export const setPriority = (): void => {};
export const tmpdir = (): string => "/tmp/";
export const totalmem = (): number => 0;
export const type = (): string => "Linux";
export const uptime = (): number => 0;
export const userInfo = (): Record<string, never> => ({});
export const version = (): string => "";

export default {
  EOL,
  arch,
  availableParallelism,
  constants,
  cpus,
  devNull,
  endianness,
  freemem,
  getPriority,
  homedir,
  hostname,
  loadavg,
  machine,
  networkInterfaces,
  platform,
  release,
  setPriority,
  tmpdir,
  totalmem,
  type,
  uptime,
  userInfo,
  version,
};
