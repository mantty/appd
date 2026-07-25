export interface RuntimeConfig {
  readonly assets?: AssetConfig;
  readonly certificates: CertificateConfig;
  readonly host: string;
  readonly port: number;
  readonly requireClientCertificate: boolean;
}

export interface AssetConfig {
  readonly manifest: string;
  readonly root: string;
}

export interface CertificateConfig {
  readonly ca: string;
  readonly identity: string;
}

export interface WorkerModule {
  fetch(
    request: Request,
    env: WorkerEnvironment,
    context: ExecutionContext,
  ): Response | Promise<Response>;
}

export interface WorkerEntrypointConstructor {
  new (context: ExecutionContext, environment: WorkerEnvironment): WorkerModule;
}

export type WorkerExport = WorkerModule | WorkerEntrypointConstructor;

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
