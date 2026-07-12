declare module "appd-worker" {
  const worker: unknown;
  export default worker;
}

declare module "bare-fetch" {
  const fetch: typeof globalThis.fetch & {
    Headers: typeof globalThis.Headers;
    Request: typeof globalThis.Request;
    Response: typeof globalThis.Response;
  };
  export default fetch;
}
