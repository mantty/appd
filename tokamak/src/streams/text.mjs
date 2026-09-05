const textEncoder = (value) => {
  const input = String(value);
  const bytes = [];
  for (let index = 0; index < input.length; index += 1) {
    let code = input.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff && index + 1 < input.length) {
      const low = input.charCodeAt(index + 1);
      if (low >= 0xdc00 && low <= 0xdfff) {
        code = 0x10000 + ((code - 0xd800) << 10) + low - 0xdc00;
        index += 1;
      }
    }
    if (code < 0x80) bytes.push(code);
    else if (code < 0x800) bytes.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f));
    else if (code < 0x10000) {
      bytes.push(0xe0 | (code >> 12), 0x80 | ((code >> 6) & 0x3f), 0x80 | (code & 0x3f));
    } else {
      bytes.push(0xf0 | (code >> 18), 0x80 | ((code >> 12) & 0x3f), 0x80 | ((code >> 6) & 0x3f), 0x80 | (code & 0x3f));
    }
  }
  return new Uint8Array(bytes);
};

const textDecoder = (value) => {
  const bytes = value instanceof Uint8Array ? value : new Uint8Array(value ?? []);
  let output = "";
  for (let index = 0; index < bytes.length;) {
    const first = bytes[index++];
    if (first < 0x80) {
      output += String.fromCharCode(first);
      continue;
    }
    let code;
    let width;
    if ((first & 0xe0) === 0xc0) {
      code = first & 0x1f;
      width = 1;
    } else if ((first & 0xf0) === 0xe0) {
      code = first & 0x0f;
      width = 2;
    } else {
      code = first & 0x07;
      width = 3;
    }
    for (let count = 0; count < width && index < bytes.length; count += 1) {
      code = (code << 6) | (bytes[index++] & 0x3f);
    }
    if (code <= 0xffff) output += String.fromCharCode(code);
    else {
      const adjusted = code - 0x10000;
      output += String.fromCharCode(0xd800 | (adjusted >> 10), 0xdc00 | (adjusted & 0x3ff));
    }
  }
  return output;
};

export class TextEncoder {
  encode(value = "") { return textEncoder(value); }
  encodeInto(source, destination) {
    const encoded = textEncoder(source);
    const written = Math.min(encoded.length, destination.length);
    destination.set(encoded.subarray(0, written));
    return { read: String(source).length, written };
  }
}

export class TextDecoder {
  constructor() {}
  decode(value = new Uint8Array()) { return textDecoder(value); }
}
