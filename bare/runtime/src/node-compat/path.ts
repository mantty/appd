import path from "bare-path";
import { format as pathFormat, matchesGlob, parse as pathParse } from "pathe";

const posix = withRoot(withCompatibility(path.posix), "/");
const win32 = withRoot(withCompatibility(path.win32), "C:\\\\");

export const {
  basename,
  delimiter,
  dirname,
  extname,
  format,
  isAbsolute,
  join,
  normalize,
  parse,
  relative,
  sep,
  toNamespacedPath,
} = posix;
export { matchesGlob };
export { posix, win32 };
export default Object.assign({}, posix, { posix, win32 });

export function resolve(...paths: string[]): string {
  return posix.resolve(...paths);
}

interface PathModule {
  readonly basename: typeof path.basename;
  readonly delimiter: typeof path.delimiter;
  readonly dirname: typeof path.dirname;
  readonly extname: typeof path.extname;
  readonly format: typeof pathFormat;
  readonly isAbsolute: typeof path.isAbsolute;
  readonly join: typeof path.join;
  readonly matchesGlob: typeof matchesGlob;
  readonly normalize: typeof path.normalize;
  readonly parse: typeof pathParse;
  readonly relative: typeof path.relative;
  readonly sep: typeof path.sep;
  readonly toNamespacedPath: typeof path.toNamespacedPath;
  resolve(...paths: string[]): string;
}

function withCompatibility(module: typeof path): PathModule {
  return Object.assign({}, module, { format: pathFormat, matchesGlob, parse: pathParse });
}

function withRoot(module: PathModule, root: string): PathModule {
  return Object.assign({}, module, {
    resolve: (...paths: string[]): string => module.resolve(root, ...paths),
  });
}
