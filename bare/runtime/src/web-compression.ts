import Stream from "bare-stream";
import * as zlib from "bare-zlib";

type Format = "deflate" | "deflate-raw" | "gzip";
type WebDuplex = {
  readonly readable: ReadableStream<Uint8Array>;
  readonly writable: WritableStream<Uint8Array>;
};

export class CompressionStream {
  readonly readable: ReadableStream<Uint8Array>;
  readonly writable: WritableStream<Uint8Array>;

  constructor(format: Format) {
    ({ readable: this.readable, writable: this.writable } = webDuplex(compressor(format)));
  }
}

export class DecompressionStream {
  readonly readable: ReadableStream<Uint8Array>;
  readonly writable: WritableStream<Uint8Array>;

  constructor(format: Format) {
    ({ readable: this.readable, writable: this.writable } = webDuplex(decompressor(format)));
  }
}

function compressor(format: Format): InstanceType<typeof Stream.Duplex> {
  switch (format) {
    case "deflate": return zlib.createDeflate();
    case "deflate-raw": return zlib.createDeflateRaw();
    case "gzip": return zlib.createGzip();
    default: throw new TypeError(`Unsupported compression format: ${format}`);
  }
}

function decompressor(format: Format): InstanceType<typeof Stream.Duplex> {
  switch (format) {
    case "deflate": return zlib.createInflate();
    case "deflate-raw": return zlib.createInflateRaw();
    case "gzip": return zlib.createGunzip();
    default: throw new TypeError(`Unsupported compression format: ${format}`);
  }
}

function webDuplex(stream: InstanceType<typeof Stream.Duplex>): WebDuplex {
  return Stream.Duplex.toWeb(stream) as unknown as WebDuplex;
}
