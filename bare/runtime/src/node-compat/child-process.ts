import { unsupportedClass, unsupportedMethod } from "./not-implemented.js";

export const ChildProcess = unsupportedClass("child_process", "ChildProcess");
export const _forkChild = unsupportedMethod("child_process", "_forkChild");
export const exec = unsupportedMethod("child_process", "exec");
export const execFile = unsupportedMethod("child_process", "execFile");
export const execFileSync = unsupportedMethod("child_process", "execFileSync");
export const execSync = unsupportedMethod("child_process", "execSync");
export const fork = unsupportedMethod("child_process", "fork");
export const spawn = unsupportedMethod("child_process", "spawn");
export const spawnSync = unsupportedMethod("child_process", "spawnSync");

export default {
  ChildProcess,
  _forkChild,
  exec,
  execFile,
  execFileSync,
  execSync,
  fork,
  spawn,
  spawnSync,
};
