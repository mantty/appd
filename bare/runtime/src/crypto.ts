import Buffer from "bare-buffer";
import nativeCrypto from "bare-crypto";

type Subtle = {
  decrypt?: (algorithm: AlgorithmIdentifier, key: CryptoKey, data: BufferSource) => Promise<ArrayBuffer>;
  encrypt?: (algorithm: AlgorithmIdentifier, key: CryptoKey, data: BufferSource) => Promise<ArrayBuffer>;
  importKey: (
    format: KeyFormat,
    keyData: BufferSource | JsonWebKey,
    algorithm: AlgorithmIdentifier,
    extractable: boolean,
    usages: KeyUsage[],
  ) => Promise<CryptoKey>;
};

type CryptoWithSubtle = { subtle: Subtle };

interface Cipher {
  final: () => Buffer;
  getAuthTag: () => Buffer;
  setAAD: (data: Buffer) => void;
  setAuthTag: (tag: Buffer) => void;
  update: (data: Buffer) => Buffer;
}

interface NativeCrypto {
  createCipheriv: (algorithm: string, key: Buffer, iv: Buffer, options: { authTagLength: number }) => Cipher;
  createDecipheriv: (algorithm: string, key: Buffer, iv: Buffer, options: { authTagLength: number }) => Cipher;
}

interface AesGcmParams {
  additionalData?: BufferSource;
  iv: BufferSource;
  name: string;
  tagLength?: number;
}

interface AesGcmKey {
  algorithm: { length: number; name: "AES-GCM" };
  extractable: boolean;
  type: "secret";
  usages: KeyUsage[];
}

const aesKeys = new WeakMap<object, Buffer>();
const native = nativeCrypto as unknown as NativeCrypto;

/** Add AES-GCM support to Bare's WebCrypto implementation. */
export function installAesGcm(crypto: CryptoWithSubtle): void {
  const subtle = crypto.subtle;
  const importKey = subtle.importKey.bind(subtle);
  const encrypt = subtle.encrypt?.bind(subtle);
  const decrypt = subtle.decrypt?.bind(subtle);

  subtle.importKey = async (format, keyData, algorithm, extractable, usages) => {
    if (!isAesGcm(algorithm)) {
      return importKey(format, keyData, algorithm, extractable, usages);
    }
    if (format !== "raw" || !isBufferSource(keyData)) {
      throw new TypeError("AES-GCM keys must use raw key data");
    }
    const data = toBuffer(keyData);
    const length = data.byteLength * 8;
    if (length !== 128 && length !== 256) {
      throw new TypeError("AES-GCM keys must be 128 or 256 bits");
    }
    const key = Object.freeze<AesGcmKey>({
      algorithm: { name: "AES-GCM", length },
      extractable,
      type: "secret",
      usages: [...usages],
    });
    aesKeys.set(key, data);
    return key;
  };

  subtle.encrypt = async (algorithm, key, data) => {
    const secret = aesKeys.get(key);
    if (secret === undefined || !isAesGcm(algorithm)) {
      if (encrypt === undefined) throw new TypeError("Unsupported encryption algorithm");
      return encrypt(algorithm, key, data);
    }
    requireUsage(key, "encrypt");
    return seal(secret, parameters(algorithm), toBuffer(data));
  };

  subtle.decrypt = async (algorithm, key, data) => {
    const secret = aesKeys.get(key);
    if (secret === undefined || !isAesGcm(algorithm)) {
      if (decrypt === undefined) throw new TypeError("Unsupported decryption algorithm");
      return decrypt(algorithm, key, data);
    }
    requireUsage(key, "decrypt");
    return open(secret, parameters(algorithm), toBuffer(data));
  };
}

function seal(key: Buffer, algorithm: AesGcmParams, data: Buffer): ArrayBuffer {
  const cipher = native.createCipheriv(cipherName(key), key, toBuffer(algorithm.iv), {
    authTagLength: tagLength(algorithm),
  });
  if (algorithm.additionalData !== undefined) cipher.setAAD(toBuffer(algorithm.additionalData));
  return toArrayBuffer(Buffer.concat([cipher.update(data), cipher.final(), cipher.getAuthTag()]));
}

function open(key: Buffer, algorithm: AesGcmParams, data: Buffer): ArrayBuffer {
  const length = tagLength(algorithm);
  if (data.byteLength < length) throw new TypeError("AES-GCM data is shorter than its tag");
  const cipherText = Buffer.from(data.subarray(0, -length));
  const tag = Buffer.from(data.subarray(-length));
  const decipher = native.createDecipheriv(cipherName(key), key, toBuffer(algorithm.iv), {
    authTagLength: length,
  });
  if (algorithm.additionalData !== undefined) decipher.setAAD(toBuffer(algorithm.additionalData));
  decipher.setAuthTag(tag);
  return toArrayBuffer(Buffer.concat([decipher.update(cipherText), decipher.final()]));
}

function parameters(algorithm: AlgorithmIdentifier): AesGcmParams {
  if (!isAesGcmParameters(algorithm)) {
    throw new TypeError("AES-GCM requires an initialization vector");
  }
  return algorithm;
}

function tagLength(algorithm: AesGcmParams): number {
  const bits = algorithm.tagLength ?? 128;
  if (bits !== 96 && bits !== 112 && bits !== 128) {
    throw new TypeError("AES-GCM tag length must be 96, 112, or 128 bits");
  }
  return bits / 8;
}

function cipherName(key: Buffer): "aes-128-gcm" | "aes-256-gcm" {
  return key.byteLength === 16 ? "aes-128-gcm" : "aes-256-gcm";
}

function requireUsage(key: CryptoKey, usage: KeyUsage): void {
  if (!key.usages.includes(usage)) throw new TypeError(`AES-GCM key cannot ${usage}`);
}

function isAesGcm(algorithm: AlgorithmIdentifier): boolean {
  return (typeof algorithm === "string" ? algorithm : algorithm.name).toUpperCase() === "AES-GCM";
}

function isAesGcmParameters(algorithm: AlgorithmIdentifier): algorithm is AesGcmParams {
  return typeof algorithm !== "string" && isAesGcm(algorithm) && "iv" in algorithm;
}

function isBufferSource(value: BufferSource | JsonWebKey): value is BufferSource {
  return value instanceof ArrayBuffer || ArrayBuffer.isView(value);
}

function toBuffer(source: BufferSource): Buffer {
  if (source instanceof ArrayBuffer) return Buffer.from(new Uint8Array(source));
  return Buffer.from(new Uint8Array(source.buffer, source.byteOffset, source.byteLength));
}

function toArrayBuffer(buffer: Buffer): ArrayBuffer {
  return buffer.buffer.slice(buffer.byteOffset, buffer.byteOffset + buffer.byteLength);
}
