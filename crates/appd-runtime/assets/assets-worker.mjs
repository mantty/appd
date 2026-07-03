const ALLOWED_METHODS = new Set(["GET", "HEAD"]);
const DEFAULT_MANIFEST = {
  files: {},
  htmlHandling: "auto-trailing-slash",
  notFoundHandling: "none",
};

export default {
  async fetch(request, env) {
    const assetRequest = new Request(request);
    if (!ALLOWED_METHODS.has(assetRequest.method)) {
      return new Response("Method Not Allowed", {
        status: 405,
        headers: { allow: "GET, HEAD" },
      });
    }

    const manifest = normalizeManifest(env.__ASSET_MANIFEST);
    const key = requestKey(new URL(assetRequest.url).pathname);
    if (key === null) {
      return notFound(assetRequest, env, manifest, "");
    }

    const resolved = resolveAssetKey(key, manifest);
    if (resolved !== null) {
      return serveAsset(assetRequest, env, manifest, resolved, 200);
    }

    return notFound(assetRequest, env, manifest, key);
  },
};

function normalizeManifest(manifest) {
  if (!manifest || typeof manifest !== "object") {
    return DEFAULT_MANIFEST;
  }

  return {
    files: manifest.files && typeof manifest.files === "object" ? manifest.files : {},
    htmlHandling: manifest.htmlHandling ?? DEFAULT_MANIFEST.htmlHandling,
    notFoundHandling: manifest.notFoundHandling ?? DEFAULT_MANIFEST.notFoundHandling,
  };
}

function requestKey(pathname) {
  if (!pathname.startsWith("/")) {
    return null;
  }

  const decoded = [];
  for (const segment of pathname.slice(1).split("/")) {
    let value;
    try {
      value = decodeURIComponent(segment);
    } catch {
      return null;
    }
    if (value === "." || value === ".." || value.includes("/") || value.includes("\\")) {
      return null;
    }
    decoded.push(value);
  }

  return decoded.join("/");
}

function resolveAssetKey(key, manifest) {
  for (const candidate of candidateKeys(key, manifest.htmlHandling)) {
    if (Object.hasOwn(manifest.files, candidate)) {
      return candidate;
    }
  }
  return null;
}

function candidateKeys(key, htmlHandling) {
  const exact = key.replace(/^\/+/, "");
  const withoutTrailingSlash = exact.replace(/\/+$/, "");
  const candidates = [exact];

  if (exact === "") {
    candidates.push("index.html");
  } else if (htmlHandling === "force-trailing-slash") {
    candidates.push(`${withoutTrailingSlash}/index.html`, `${withoutTrailingSlash}.html`);
  } else if (htmlHandling === "drop-trailing-slash") {
    candidates.push(`${withoutTrailingSlash}.html`, `${withoutTrailingSlash}/index.html`);
  } else if (exact.endsWith("/")) {
    candidates.push(`${withoutTrailingSlash}/index.html`);
  } else {
    candidates.push(`${withoutTrailingSlash}.html`, `${withoutTrailingSlash}/index.html`);
  }

  return candidates.filter(Boolean);
}

async function notFound(request, env, manifest, key) {
  if (manifest.notFoundHandling === "single-page-application") {
    const rootKey = resolveAssetKey("", manifest);
    if (rootKey !== null) {
      return serveAsset(request, env, manifest, rootKey, 200);
    }
  }

  if (manifest.notFoundHandling === "404-page") {
    const notFoundKey = resolve404Key(key, manifest);
    if (notFoundKey !== null) {
      return serveAsset(request, env, manifest, notFoundKey, 404);
    }
  }

  return new Response(request.method === "HEAD" ? null : "Not Found", { status: 404 });
}

function resolve404Key(key, manifest) {
  const parts = key.replace(/\/+$/, "").split("/").filter(Boolean);
  for (let length = parts.length; length >= 0; length -= 1) {
    const candidate = [...parts.slice(0, length), "404.html"].join("/");
    if (Object.hasOwn(manifest.files, candidate)) {
      return candidate;
    }
  }
  return null;
}

async function serveAsset(request, env, manifest, key, status) {
  const upstream = await env.__ASSET_FILES.fetch(assetUrl(key), { method: request.method });
  if (upstream.status === 404) {
    return upstream;
  }

  const headers = new Headers(upstream.headers);
  headers.set("content-type", manifest.files[key]);

  return new Response(request.method === "HEAD" ? null : upstream.body, {
    status,
    statusText: upstream.statusText,
    headers,
  });
}

function assetUrl(key) {
  return `http://assets.local/${key.split("/").map(encodeURIComponent).join("/")}`;
}
