import Stream from "bare-stream";

export default Stream;
export const {
  Duplex,
  PassThrough,
  Readable,
  Stream: StreamClass,
  Transform,
  Writable,
  addAbortSignal,
  duplexPair,
  finished,
  getStreamError,
  isDisturbed,
  isEnded,
  isErrored,
  isFinished,
  isReadable,
  isStream,
  isWritable,
  pipeline,
} = Stream;
export { StreamClass as Stream };
