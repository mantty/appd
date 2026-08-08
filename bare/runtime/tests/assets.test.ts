import assert from "node:assert/strict";
import test from "node:test";

import { resolveAsset } from "../src/asset-routing.js";

const manifest = {
  binding: "ASSETS",
  files: {
    "404.html": "text/html",
    "about/index.html": "text/html",
    "index.html": "text/html",
  },
  htmlHandling: "auto-trailing-slash" as const,
  notFoundHandling: "404-page" as const,
};

void test("resolves clean URLs and index pages", () => {
  assert.deepEqual(resolveAsset("/", manifest), { key: "index.html", status: 200 });
  assert.deepEqual(resolveAsset("/about", manifest), { key: "about/index.html", status: 200 });
});

void test("falls back to the configured 404 page", () => {
  assert.deepEqual(resolveAsset("/missing", manifest), { key: "404.html", status: 404 });
});

void test("uses the nearest 404 page", () => {
  const nested = { ...manifest, files: { ...manifest.files, "docs/404.html": "text/html" } };
  assert.deepEqual(resolveAsset("/docs/missing", nested), {
    key: "docs/404.html",
    status: 404,
  });
});

void test("supports every HTML routing mode", () => {
  const files = { "about.html": "text/html", "about/index.html": "text/html" };
  assert.equal(resolveAsset("/about", { ...manifest, files, htmlHandling: "none" }), undefined);
  assert.deepEqual(
    resolveAsset("/about", { ...manifest, files, htmlHandling: "drop-trailing-slash" }),
    { key: "about.html", status: 200 },
  );
  assert.deepEqual(
    resolveAsset("/about", { ...manifest, files, htmlHandling: "force-trailing-slash" }),
    { key: "about/index.html", status: 200 },
  );
});

void test("falls back to the application shell in SPA mode", () => {
  assert.deepEqual(resolveAsset("/route", { ...manifest, notFoundHandling: "single-page-application" }), {
    key: "index.html",
    status: 200,
  });
});

void test("rejects parent path traversal", () => {
  assert.equal(resolveAsset("/%2e%2e/private", { ...manifest, notFoundHandling: "none" }), undefined);
});

void test("rejects invalid percent encoding", () => {
  assert.equal(resolveAsset("/%GG", manifest), undefined);
});
