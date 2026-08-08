import type { ExecutionContext, WorkerEnvironment, WorkerExport } from "./types.js";

export function invokeWorker(
  worker: WorkerExport,
  request: Request,
  environment: WorkerEnvironment,
  context: ExecutionContext,
): Response | Promise<Response> {
  const instance = typeof worker === "function" ? new worker(context, environment) : worker;
  return instance.fetch(request, environment, context);
}
