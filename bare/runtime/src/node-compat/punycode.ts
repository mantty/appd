import punycode from "bare-punycode";

interface Punycode {
  readonly decode: (input: string) => string;
  readonly encode: (input: string) => string;
  readonly toASCII: (input: string) => string;
  readonly toUnicode: (input: string) => string;
  readonly ucs2: unknown;
  readonly version: string;
}

const implementation = punycode as Punycode;

export default implementation;
export const { decode, encode, toASCII, toUnicode, ucs2, version } = implementation;
