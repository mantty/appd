import Buffer from "bare-buffer";

export async function arrayBuffer(source: AsyncIterable<unknown>): Promise<ArrayBuffer> {
  const result = await buffer(source);
  return result.buffer.slice(result.byteOffset, result.byteOffset + result.byteLength);
}

export async function blob(source: AsyncIterable<unknown>): Promise<Blob> {
  return new Blob([await buffer(source)]);
}

export async function buffer(source: AsyncIterable<unknown>): Promise<Buffer> {
  const chunks: Buffer[] = [];
  for await (const chunk of source) chunks.push(toBuffer(chunk));
  return Buffer.concat(chunks);
}

export async function json(source: AsyncIterable<unknown>): Promise<unknown> {
  return JSON.parse(await text(source));
}

export async function text(source: AsyncIterable<unknown>): Promise<string> {
  return (await buffer(source)).toString("utf8");
}

export default { arrayBuffer, blob, buffer, json, text };

function toBuffer(chunk: unknown): Buffer {
  if (typeof chunk === "string") return Buffer.from(chunk);
  if (chunk instanceof Uint8Array) return Buffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength);
  throw new TypeError("A stream consumer received a non-byte chunk");
}
