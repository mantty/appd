export interface RuntimeConfig {
  readonly assets?: AssetConfig;
  readonly certificates: CertificateConfig;
  readonly port: number;
}

export interface AssetConfig {
  readonly manifest: string;
  readonly root: string;
}

export interface CertificateConfig {
  readonly ca: string;
  readonly certificate: string;
  readonly privateKey: string;
}

export interface WorkerModule {
  fetch(
    request: Request,
    env: WorkerEnvironment,
    context: ExecutionContext,
  ): Response | Promise<Response>;
}

export interface WorkerEnvironment {
  readonly ASSETS?: Fetcher;
  readonly [name: string]: unknown;
}

export interface Fetcher {
  fetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response>;
}

export interface ExecutionContext {
  passThroughOnException(): void;
  waitUntil(promise: Promise<unknown>): void;
}
