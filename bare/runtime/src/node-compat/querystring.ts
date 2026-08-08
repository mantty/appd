import { decode, encode, parse, stringify } from "bare-querystring";

export { decode, encode, parse, stringify };
export const escape = encodeURIComponent;
export const unescape = decodeURIComponent;
export default { decode, encode, escape, parse, stringify, unescape };
