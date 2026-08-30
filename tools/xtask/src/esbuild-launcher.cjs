const { spawnSync } = require("node:child_process");
const path = require("node:path");

function esbuildPath(platform, arch) {
  const hosts = ["darwin-arm64", "darwin-x64", "linux-x64", "win32-x64"];
  const host = `${platform}-${arch}`;
  if (!hosts.includes(host)) {
    throw new Error(`appd does not provide esbuild for ${host}`);
  }
  const executable = platform === "win32" ? "esbuild.exe" : "bin/esbuild";
  return path.resolve(__dirname, "../..", "@esbuild", host, executable);
}

if (require.main === module) {
  const result = spawnSync(esbuildPath(process.platform, process.arch), process.argv.slice(2), {
    stdio: "inherit",
  });
  if (result.error) {
    throw result.error;
  }
  process.exit(result.status ?? 1);
}

module.exports = { esbuildPath };
