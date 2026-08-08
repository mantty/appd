import zlib from "bare-zlib";

export const {
  Deflate,
  DeflateRaw,
  Gzip,
  Gunzip,
  Inflate,
  InflateRaw,
  constants,
  createDeflate,
  createDeflateRaw,
  createGunzip,
  createGzip,
  createInflate,
  createInflateRaw,
  deflate,
  deflateRaw,
  deflateRawSync,
  deflateSync,
  gunzip,
  gunzipSync,
  gzip,
  gzipSync,
  inflate,
  inflateRaw,
  inflateRawSync,
  inflateSync,
} = zlib;

export default zlib;
