export interface AssetManifest {
  readonly binding: string;
  readonly files: Readonly<Record<string, string>>;
  readonly htmlHandling: "auto-trailing-slash" | "drop-trailing-slash" | "force-trailing-slash" | "none";
  readonly notFoundHandling: "404-page" | "none" | "single-page-application";
}

export interface AssetResolution {
  readonly key: string;
  readonly status: number;
}

export function resolveAsset(pathname: string, manifest: AssetManifest): AssetResolution | undefined {
  const key = requestKey(pathname);
  if (key === undefined) return undefined;
  const asset = firstExisting(candidates(key, manifest.htmlHandling), manifest);
  if (asset !== undefined) return { key: asset, status: 200 };
  if (manifest.notFoundHandling === "single-page-application") {
    const index = firstExisting(candidates("", manifest.htmlHandling), manifest);
    return index === undefined ? undefined : { key: index, status: 200 };
  }
  if (manifest.notFoundHandling === "404-page") {
    const notFound = nearest404(key, manifest);
    return notFound === undefined ? undefined : { key: notFound, status: 404 };
  }
  return undefined;
}

function requestKey(pathname: string): string | undefined {
  if (!pathname.startsWith("/")) return undefined;
  const decoded: string[] = [];
  for (const segment of pathname.slice(1).split("/")) {
    let value: string;
    try {
      value = decodeURIComponent(segment);
    } catch {
      return undefined;
    }
    if (value === "." || value === ".." || value.includes("/") || value.includes("\\")) {
      return undefined;
    }
    decoded.push(value);
  }
  return decoded.join("/");
}

function candidates(key: string, handling: AssetManifest["htmlHandling"]): string[] {
  const exact = key.replace(/^\/+/, "");
  const clean = exact.replace(/\/+$/, "");
  if (exact === "") return ["index.html"];
  if (handling === "none") return [exact];
  if (handling === "force-trailing-slash") return [exact, `${clean}/index.html`, `${clean}.html`];
  if (handling === "drop-trailing-slash") return [exact, `${clean}.html`, `${clean}/index.html`];
  if (exact.endsWith("/")) return [exact, `${clean}/index.html`];
  return [exact, `${clean}.html`, `${clean}/index.html`];
}

function firstExisting(keys: string[], manifest: AssetManifest): string | undefined {
  return keys.find((key) => Object.hasOwn(manifest.files, key));
}

function nearest404(key: string, manifest: AssetManifest): string | undefined {
  const parts = key.replace(/\/+$/, "").split("/").filter(Boolean);
  for (let length = parts.length; length >= 0; length -= 1) {
    const candidate = [...parts.slice(0, length), "404.html"].join("/");
    if (Object.hasOwn(manifest.files, candidate)) return candidate;
  }
  return undefined;
}
