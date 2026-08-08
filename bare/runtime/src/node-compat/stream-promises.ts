import Stream from "bare-stream";

export function finished(stream: InstanceType<typeof Stream>): Promise<void> {
  return new Promise((resolve, reject) => {
    Stream.finished(stream, (error) => {
      if (error === null) resolve();
      else reject(asError(error));
    });
  });
}

export function pipeline(...streams: Parameters<typeof Stream.pipeline>): Promise<void> {
  return new Promise((resolve, reject) => {
    pipelineWithCallback(...streams, (error) => {
      if (error === null) resolve();
      else reject(asError(error));
    });
  });
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

const pipelineWithCallback = Stream.pipeline as unknown as (
  ...arguments_: [...Parameters<typeof Stream.pipeline>, (error: Error | null) => void]
) => void;

export default { finished, pipeline };
