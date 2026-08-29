export function installConsoleGlobal() {
  globalThis.console ??= {
    log() {}, info() {}, warn() {}, error() {}, debug() {}, trace() {}, dir() {}, time() {}, timeEnd() {},
  };
}
