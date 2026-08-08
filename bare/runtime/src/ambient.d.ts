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

declare module "bare-diagnostics-channel" {
  interface Channel {
    publish(message: unknown): void;
    subscribe(listener: (message: unknown, name: string) => void): void;
    unsubscribe(listener: (message: unknown, name: string) => void): boolean;
  }

  interface DiagnosticsChannel {
    readonly Channel: new (name: string) => Channel;
    channel(name: string): Channel;
    hasSubscribers(name: string): boolean;
    subscribe(name: string, listener: (message: unknown, name: string) => void): void;
    tracingChannel(name: string): unknown;
    unsubscribe(name: string, listener: (message: unknown, name: string) => void): boolean;
  }

  const diagnosticsChannel: DiagnosticsChannel;
  export default diagnosticsChannel;
}

declare module "bare-punycode" {
  const punycode: unknown;
  export default punycode;
}

declare module "bare-mime" {
  interface MIME {
    readonly parameters: ReadonlyMap<string, string>;
    readonly subtype: string;
    readonly type: string;
  }

  export function parse(value: string): MIME | null;
}
