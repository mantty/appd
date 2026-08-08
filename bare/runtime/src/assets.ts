import fs from "bare-fs";
import { Readable } from "bare-stream";

import { resolveAsset, type AssetManifest } from "./asset-routing.js";
import type { AssetConfig, Fetcher } from "./types.js";

export class AssetService implements Fetcher {
  readonly #config: AssetConfig;
  readonly #manifest: AssetManifest;

  constructor(config: AssetConfig) {
    this.#config = config;
    this.#manifest = JSON.parse(fs.readFileSync(config.manifest, "utf8")) as AssetManifest;
  }

  get binding(): string {
    return this.#manifest.binding;
  }

  fetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
    const request = input instanceof Request ? input : new Request(input, init);
    if (request.method !== "GET" && request.method !== "HEAD") {
      return Promise.resolve(new Response("Method Not Allowed", {
        headers: { allow: "GET, HEAD" },
        status: 405,
      }));
    }

    const resolution = resolveAsset(new URL(request.url).pathname, this.#manifest);
    if (resolution === undefined) return Promise.resolve(new Response("Not Found", { status: 404 }));

    const type = this.#manifest.files[resolution.key];
    const body = request.method === "HEAD"
      ? null
      : Readable.toWeb(fs.createReadStream(
        `${this.#config.root}/${resolution.key}`,
        { eagerOpen: false } as Parameters<typeof fs.createReadStream>[1],
      )) as unknown as BodyInit;
    return Promise.resolve(new Response(body, {
      headers: { "content-type": type ?? "application/octet-stream" },
      status: resolution.status,
    }));
  }
}
