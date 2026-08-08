import {
  AssertionError,
  deepStrictEqual,
  doesNotMatch,
  doesNotReject,
  doesNotThrow,
  fail,
  ifError,
  match,
  notDeepStrictEqual,
  notStrictEqual,
  ok,
  rejects,
  strictEqual,
  throws,
  strict,
} from "./assert.js";

export default strict;
export {
  AssertionError,
  deepStrictEqual,
  doesNotMatch,
  doesNotReject,
  doesNotThrow,
  fail,
  ifError,
  match,
  notDeepStrictEqual,
  ok,
  rejects,
  strictEqual,
  throws,
};
export const equal = strictEqual;
export const deepEqual = deepStrictEqual;
export const notEqual = notStrictEqual;
export const notDeepEqual = notDeepStrictEqual;
