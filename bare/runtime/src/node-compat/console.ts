import Console from "bare-console";

interface ConsoleWithGroups extends Console {
  group(...arguments_: unknown[]): void;
  groupCollapsed(...arguments_: unknown[]): void;
  groupEnd(): void;
}

const implementation = Object.assign(new Console(), {
  group(...arguments_: unknown[]): void {
    implementation.log(...arguments_);
  },
  groupCollapsed(...arguments_: unknown[]): void {
    implementation.log(...arguments_);
  },
  groupEnd(): void {},
}) as ConsoleWithGroups;

export { Console };
export const {
  assert,
  clear,
  count,
  countReset,
  debug,
  error,
  group,
  groupCollapsed,
  groupEnd,
  info,
  log,
  table,
  time,
  timeEnd,
  timeLog,
  trace,
  warn,
} = implementation;
export default implementation;
