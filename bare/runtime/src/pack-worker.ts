import { spawnSync } from "node:child_process";
import { realpathSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { compilerAliases } from "./node-compat/registry.js";

interface Options {
  readonly compiler: string;
  readonly output: string;
  readonly worker: string;
}

export function compilerArguments(options: Options, runtimeDirectory: string): string[] {
  return [
    "--bundle",
    "--format=cjs",
    "--platform=neutral",
    "--target=safari15",
    "--packages=external",
    `--alias:appd-worker=${options.worker}`,
    `--alias:cloudflare:node=${join(runtimeDirectory, "cloudflare-node.js")}`,
    `--alias:cloudflare:sockets=${join(runtimeDirectory, "sockets.js")}`,
    `--alias:cloudflare:workers=${join(runtimeDirectory, "cloudflare.js")}`,
    ...compilerAliases((path) => join(runtimeDirectory, path)),
    `--outfile=${options.output}`,
    join(runtimeDirectory, "entry.js"),
  ];
}

function options(): Options {
  const { values } = parseArgs({
    options: {
      compiler: { type: "string" },
      output: { type: "string" },
      worker: { type: "string" },
    },
  });
  return {
    compiler: required(values.compiler, "compiler"),
    output: required(values.output, "output"),
    worker: required(values.worker, "worker"),
  };
}

function required(value: string | boolean | undefined, name: string): string {
  if (typeof value === "string") return value;
  throw new Error(`--${name} is required`);
}

function main(): void {
  const runtimeDirectory = dirname(fileURLToPath(import.meta.url));
  const build = options();
  const result = spawnSync(
    build.compiler,
    compilerArguments(build, runtimeDirectory),
    { stdio: "inherit" },
  );
  if (result.error !== undefined) throw result.error;
  if (result.status !== 0) process.exitCode = result.status ?? 1;
}

if (process.argv[1] !== undefined && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
