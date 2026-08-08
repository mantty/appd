import bareAssert, { AssertionError } from "bare-assert";

import { isDeepEqual, isDeepStrictEqual } from "./deep-equal.js";

type AssertMessage = Error | string | undefined;

function assertionError(
  actual: unknown,
  expected: unknown,
  operator: string,
  message: AssertMessage,
): never {
  if (message instanceof Error) throw message;
  if (message !== undefined) throw new AssertionError({ actual, expected, message, operator });
  try {
    throw new AssertionError({ actual, expected, operator });
  } catch (error) {
    if (error instanceof AssertionError) throw error;
    throw new AssertionError({ actual, expected, message: `Expected ${operator} assertion to pass`, operator });
  }
}

function ok(value: unknown, message?: AssertMessage): void {
  bareAssert(value, message);
}

function equal(actual: unknown, expected: unknown, message?: AssertMessage): void {
  if (actual == expected) return;
  assertionError(actual, expected, "==", message);
}

function notEqual(actual: unknown, expected: unknown, message?: AssertMessage): void {
  if (actual != expected) return;
  assertionError(actual, expected, "!=", message);
}

function strictEqual(actual: unknown, expected: unknown, message?: AssertMessage): void {
  if (Object.is(actual, expected)) return;
  assertionError(actual, expected, "strictEqual", message);
}

function notStrictEqual(actual: unknown, expected: unknown, message?: AssertMessage): void {
  if (!Object.is(actual, expected)) return;
  assertionError(actual, expected, "notStrictEqual", message);
}

function deepStrictEqual(actual: unknown, expected: unknown, message?: AssertMessage): void {
  if (isDeepStrictEqual(actual, expected)) return;
  assertionError(actual, expected, "deepStrictEqual", message);
}

function notDeepStrictEqual(actual: unknown, expected: unknown, message?: AssertMessage): void {
  if (!isDeepStrictEqual(actual, expected)) return;
  assertionError(actual, expected, "notDeepStrictEqual", message);
}

function deepEqual(actual: unknown, expected: unknown, message?: AssertMessage): void {
  if (isDeepEqual(actual, expected)) return;
  assertionError(actual, expected, "deepEqual", message);
}

function notDeepEqual(actual: unknown, expected: unknown, message?: AssertMessage): void {
  if (!isDeepEqual(actual, expected)) return;
  assertionError(actual, expected, "notDeepEqual", message);
}

function partialDeepStrictEqual(actual: unknown, expected: unknown, message?: AssertMessage): void {
  if (partiallyEqual(actual, expected)) return;
  assertionError(actual, expected, "partialDeepStrictEqual", message);
}

interface PartialComparison {
  actual: object[];
  expected: object[];
}

function partiallyEqual(
  actual: unknown,
  expected: unknown,
  compared: PartialComparison = { actual: [], expected: [] },
): boolean {
  if (Object.is(actual, expected)) return true;
  if (actual === null || expected === null) return false;
  if (typeof actual !== "object" || typeof expected !== "object") return false;
  if (Object.getPrototypeOf(actual) !== Object.getPrototypeOf(expected)) return false;

  const actualIndex = compared.actual.indexOf(actual);
  if (actualIndex !== -1) return compared.expected[actualIndex] === expected;
  if (compared.expected.includes(expected)) return false;
  compared.actual.push(actual);
  compared.expected.push(expected);

  if (!partiallyEqualBuiltin(actual, expected, compared)) return false;
  const expectedObject = expected as Record<PropertyKey, unknown>;
  const actualObject = actual as Record<PropertyKey, unknown>;
  return enumerableKeys(expectedObject).every((key) => (
    Object.hasOwn(actualObject, key) && partiallyEqual(actualObject[key], expectedObject[key], compared)
  ));
}

function enumerableKeys(value: object): PropertyKey[] {
  return Reflect.ownKeys(value).filter((key) => Object.prototype.propertyIsEnumerable.call(value, key));
}

function partiallyEqualBuiltin(actual: object, expected: object, compared: PartialComparison): boolean {
  if (actual instanceof Date && expected instanceof Date) return actual.getTime() === expected.getTime();
  if (actual instanceof RegExp && expected instanceof RegExp) {
    return actual.source === expected.source && actual.flags === expected.flags && actual.lastIndex === expected.lastIndex;
  }
  if (actual instanceof Error && expected instanceof Error) {
    return actual.name === expected.name
      && actual.message === expected.message
      && (expected.cause === undefined || partiallyEqual(actual.cause, expected.cause, compared));
  }
  if (actual instanceof Map && expected instanceof Map) return partiallyEqualMap(actual, expected, compared);
  if (actual instanceof Set && expected instanceof Set) return partiallyEqualSet(actual, expected, compared);
  if (actual instanceof ArrayBuffer && expected instanceof ArrayBuffer) return partiallyEqualBytes(actual, expected);
  if (typeof SharedArrayBuffer !== "undefined" && actual instanceof SharedArrayBuffer && expected instanceof SharedArrayBuffer) {
    return partiallyEqualBytes(actual, expected);
  }
  if (ArrayBuffer.isView(actual) && ArrayBuffer.isView(expected)) return partiallyEqualBytes(actual, expected);
  if (Array.isArray(actual) && Array.isArray(expected)) return actual.length >= expected.length;
  const tag = Object.prototype.toString.call(actual);
  if (tag === "[object WeakMap]" || tag === "[object WeakSet]") return false;
  if (isBoxedPrimitive(tag)) return Object.is(actual.valueOf(), expected.valueOf());
  return true;
}

function partiallyEqualMap(actual: Map<unknown, unknown>, expected: Map<unknown, unknown>, compared: PartialComparison): boolean {
  if (actual.size < expected.size) return false;
  const remaining = [...actual];
  return [...expected].every(([expectedKey, expectedValue]) => removePartialMatch(
    remaining,
    ([actualKey, actualValue]) => matchMapEntry(actualKey, actualValue, expectedKey, expectedValue, compared),
  ));
}

function partiallyEqualSet(actual: Set<unknown>, expected: Set<unknown>, compared: PartialComparison): boolean {
  if (actual.size < expected.size) return false;
  const remaining = [...actual];
  return [...expected].every((expectedValue) => removePartialMatch(
    remaining,
    (actualValue) => matchSetValue(actualValue, expectedValue, compared),
  ));
}

function matchMapEntry(
  actualKey: unknown,
  actualValue: unknown,
  expectedKey: unknown,
  expectedValue: unknown,
  compared: PartialComparison,
): boolean {
  const trial = copyComparison(compared);
  if (!partiallyEqual(actualKey, expectedKey, trial) || !partiallyEqual(actualValue, expectedValue, trial)) {
    return false;
  }
  compared.actual = trial.actual;
  compared.expected = trial.expected;
  return true;
}

function matchSetValue(actual: unknown, expected: unknown, compared: PartialComparison): boolean {
  const trial = copyComparison(compared);
  if (!partiallyEqual(actual, expected, trial)) return false;
  compared.actual = trial.actual;
  compared.expected = trial.expected;
  return true;
}

function copyComparison(comparison: PartialComparison): PartialComparison {
  return { actual: [...comparison.actual], expected: [...comparison.expected] };
}

function removePartialMatch<Value>(values: Value[], matches: (value: Value) => boolean): boolean {
  const index = values.findIndex(matches);
  if (index === -1) return false;
  values.splice(index, 1);
  return true;
}

function partiallyEqualBytes(
  actual: BufferSource,
  expected: BufferSource,
): boolean {
  const actualBytes = bytes(actual);
  const expectedBytes = bytes(expected);
  return actualBytes.byteLength >= expectedBytes.byteLength
    && expectedBytes.every((value, index) => actualBytes[index] === value);
}

type BufferSource = ArrayBuffer | SharedArrayBuffer | ArrayBufferView;

function bytes(value: BufferSource): Uint8Array {
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  if (typeof SharedArrayBuffer !== "undefined" && value instanceof SharedArrayBuffer) return new Uint8Array(value);
  const view = value as ArrayBufferView;
  return new Uint8Array(view.buffer, view.byteOffset, view.byteLength);
}

function isBoxedPrimitive(tag: string): boolean {
  return tag === "[object BigInt]"
    || tag === "[object Boolean]"
    || tag === "[object Number]"
    || tag === "[object String]"
    || tag === "[object Symbol]";
}

function match(value: string, regularExpression: RegExp, message?: AssertMessage): void {
  if (regularExpression.test(value)) return;
  assertionError(value, regularExpression, "match", message);
}

function doesNotMatch(value: string, regularExpression: RegExp, message?: AssertMessage): void {
  if (!regularExpression.test(value)) return;
  assertionError(value, regularExpression, "doesNotMatch", message);
}

function ifError(error: unknown): void {
  if (error === null || error === undefined) return;
  throw error;
}

function throws(
  block: () => unknown,
  expected?: ErrorMatcher | AssertMessage,
  message?: AssertMessage,
): void {
  try {
    block();
  } catch (error: unknown) {
    if (matches(error, expected)) return;
    assertionError(error, expected, "throws", message);
  }
  assertionError(undefined, expected, "throws", message);
}

function doesNotThrow(
  block: () => unknown,
  expected?: ErrorMatcher | AssertMessage,
  message?: AssertMessage,
): void {
  try {
    block();
  } catch (error: unknown) {
    if (!matches(error, expected)) throw error;
    assertionError(error, undefined, "doesNotThrow", message);
  }
}

async function rejects(
  block: (() => Promise<unknown>) | Promise<unknown>,
  expected?: ErrorMatcher | AssertMessage,
  message?: AssertMessage,
): Promise<void> {
  try {
    await (typeof block === "function" ? block() : block);
  } catch (error: unknown) {
    if (matches(error, expected)) return;
    assertionError(error, expected, "rejects", message);
  }
  assertionError(undefined, expected, "rejects", message);
}

async function doesNotReject(
  block: (() => Promise<unknown>) | Promise<unknown>,
  expected?: ErrorMatcher | AssertMessage,
  message?: AssertMessage,
): Promise<void> {
  try {
    await (typeof block === "function" ? block() : block);
  } catch (error: unknown) {
    if (!matches(error, expected)) throw error;
    assertionError(error, undefined, "doesNotReject", message);
  }
}

type ErrorMatcher = ((error: unknown) => unknown) | RegExp | Error | Record<string, unknown>;

function matches(error: unknown, expected: ErrorMatcher | AssertMessage): boolean {
  if (expected === undefined || typeof expected === "string") return true;
  if (expected instanceof RegExp) return expected.test(String(error));
  if (expected instanceof Error) return matchesError(error, expected);
  if (typeof expected === "function") return matchesFunction(error, expected);
  return partiallyEqual(error, expected);
}

function matchesError(error: unknown, expected: Error): boolean {
  if (!(error instanceof Error)) return false;
  if (error.name !== expected.name || error.message !== expected.message) return false;
  return Object.keys(expected).every((key) => (
    isDeepStrictEqual(error[key as keyof Error], expected[key as keyof Error])
  ));
}

function matchesFunction(error: unknown, expected: (error: unknown) => unknown): boolean {
  const constructor = expected as unknown as new () => object;
  const hasPrototype = typeof expected.prototype === "object" && expected.prototype !== null;
  if (hasPrototype && error instanceof constructor) return true;
  if (isClass(expected)) return false;
  return Reflect.apply(expected, undefined, [error]) === true;
}

function isClass(value: Function): boolean {
  return Function.prototype.toString.call(value).startsWith("class ");
}

function fail(
  actual?: unknown,
  expected?: unknown,
  message?: AssertMessage,
  operator = "fail",
): never {
  if (arguments.length === 1) assertionError(undefined, undefined, operator, actual as AssertMessage);
  return assertionError(actual, expected, operator, message);
}

const strict = (value: unknown, message?: AssertMessage): void => ok(value, message);
Object.assign(strict, {
  AssertionError,
  deepEqual: deepStrictEqual,
  deepStrictEqual,
  doesNotMatch,
  doesNotReject,
  doesNotThrow,
  equal: strictEqual,
  fail,
  ifError,
  match,
  notDeepEqual: notDeepStrictEqual,
  notDeepStrictEqual,
  notEqual: notStrictEqual,
  notStrictEqual,
  ok,
  partialDeepStrictEqual,
  rejects,
  strict,
  strictEqual,
  throws,
});

const assert = (value: unknown, message?: AssertMessage): void => ok(value, message);
Object.assign(assert, {
  AssertionError,
  deepEqual,
  deepStrictEqual,
  doesNotMatch,
  doesNotReject,
  doesNotThrow,
  equal,
  fail,
  ifError,
  match,
  notDeepEqual,
  notDeepStrictEqual,
  notEqual,
  notStrictEqual,
  ok,
  partialDeepStrictEqual,
  rejects,
  strict,
  strictEqual,
  throws,
});

export default assert;
export {
  AssertionError,
  deepEqual,
  deepStrictEqual,
  doesNotMatch,
  doesNotReject,
  doesNotThrow,
  equal,
  fail,
  ifError,
  match,
  notDeepStrictEqual,
  notDeepEqual,
  notEqual,
  notStrictEqual,
  ok,
  partialDeepStrictEqual,
  rejects,
  strict,
  strictEqual,
  throws,
};
