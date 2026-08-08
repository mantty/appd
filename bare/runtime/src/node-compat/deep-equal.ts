interface ComparedValues {
  actual: object[];
  expected: object[];
}

function isObject(value: unknown): value is object {
  return value !== null && typeof value === "object";
}

function enumerableKeys(value: object): PropertyKey[] {
  return Reflect.ownKeys(value).filter((key) => Object.prototype.propertyIsEnumerable.call(value, key));
}

function copyComparedValues(values: ComparedValues): ComparedValues {
  return { actual: [...values.actual], expected: [...values.expected] };
}

function compareBytes(actual: Uint8Array, expected: Uint8Array): boolean {
  if (actual.byteLength !== expected.byteLength) return false;
  return actual.every((value, index) => value === expected[index]);
}

function compareProperties(
  actual: Record<PropertyKey, unknown>,
  expected: Record<PropertyKey, unknown>,
  strict: boolean,
  compared: ComparedValues,
): boolean {
  const actualKeys = enumerableKeys(actual);
  const expectedKeys = enumerableKeys(expected);
  if (actualKeys.length !== expectedKeys.length) return false;

  return actualKeys.every((key) => (
    Object.hasOwn(expected, key) && compare(actual[key], expected[key], strict, compared)
  ));
}

function compareUnordered(
  actual: Iterable<unknown>,
  expected: Iterable<unknown>,
  strict: boolean,
  compared: ComparedValues,
): boolean {
  const remaining = [...expected];

  for (const actualValue of actual) {
    const index = remaining.findIndex((expectedValue) => {
      const trial = copyComparedValues(compared);
      if (!compare(actualValue, expectedValue, strict, trial)) return false;
      compared.actual = trial.actual;
      compared.expected = trial.expected;
      return true;
    });
    if (index === -1) return false;
    remaining.splice(index, 1);
  }

  return true;
}

function compareBuiltins(
  actual: object,
  expected: object,
  strict: boolean,
  compared: ComparedValues,
): boolean {
  const tag = Object.prototype.toString.call(actual);
  if (tag === "[object WeakMap]" || tag === "[object WeakSet]") return false;
  if (isBoxedPrimitive(tag)) return Object.is(actual.valueOf(), expected.valueOf());

  if (actual instanceof Date && expected instanceof Date) {
    return actual.getTime() === expected.getTime();
  }

  if (actual instanceof RegExp && expected instanceof RegExp) {
    return actual.source === expected.source && actual.flags === expected.flags && actual.lastIndex === expected.lastIndex;
  }

  if (actual instanceof Error && expected instanceof Error) {
    return actual.name === expected.name
      && actual.message === expected.message
      && compare(actual.cause, expected.cause, strict, compared);
  }

  if (actual instanceof Map && expected instanceof Map) {
    return actual.size === expected.size && compareUnordered(actual, expected, strict, compared);
  }

  if (actual instanceof Set && expected instanceof Set) {
    return actual.size === expected.size && compareUnordered(actual, expected, strict, compared);
  }

  if (actual instanceof ArrayBuffer && expected instanceof ArrayBuffer) {
    return compareBytes(new Uint8Array(actual), new Uint8Array(expected));
  }

  if (typeof SharedArrayBuffer !== "undefined" && actual instanceof SharedArrayBuffer && expected instanceof SharedArrayBuffer) {
    return compareBytes(new Uint8Array(actual), new Uint8Array(expected));
  }

  if (ArrayBuffer.isView(actual) && ArrayBuffer.isView(expected)) {
    return compareBytes(
      new Uint8Array(actual.buffer, actual.byteOffset, actual.byteLength),
      new Uint8Array(expected.buffer, expected.byteOffset, expected.byteLength),
    );
  }

  if (Array.isArray(actual) && Array.isArray(expected)) return actual.length === expected.length;
  return tag === Object.prototype.toString.call(expected);
}

function isBoxedPrimitive(tag: string): boolean {
  return tag === "[object BigInt]"
    || tag === "[object Boolean]"
    || tag === "[object Number]"
    || tag === "[object String]"
    || tag === "[object Symbol]";
}

function compare(actual: unknown, expected: unknown, strict: boolean, compared: ComparedValues): boolean {
  if (Object.is(actual, expected)) return true;
  if (!isObject(actual) || !isObject(expected)) return strict ? false : actual == expected;
  if (strict && Object.getPrototypeOf(actual) !== Object.getPrototypeOf(expected)) return false;
  if (Object.prototype.toString.call(actual) !== Object.prototype.toString.call(expected)) return false;

  const actualIndex = compared.actual.indexOf(actual);
  if (actualIndex !== -1) return compared.expected[actualIndex] === expected;
  if (compared.expected.includes(expected)) return false;

  compared.actual.push(actual);
  compared.expected.push(expected);
  if (!compareBuiltins(actual, expected, strict, compared)) return false;
  return compareProperties(
    actual as Record<PropertyKey, unknown>,
    expected as Record<PropertyKey, unknown>,
    strict,
    compared,
  );
}

export function isDeepStrictEqual(actual: unknown, expected: unknown): boolean {
  return compare(actual, expected, true, { actual: [], expected: [] });
}

export function isDeepEqual(actual: unknown, expected: unknown): boolean {
  return compare(actual, expected, false, { actual: [], expected: [] });
}
