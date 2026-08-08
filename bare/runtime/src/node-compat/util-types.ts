function tag(value: unknown): string {
  return Object.prototype.toString.call(value);
}

function isTypedArray(value: unknown): boolean {
  return ArrayBuffer.isView(value) && !(value instanceof DataView);
}

export const isExternal = (): boolean => false;
export const isDate = (value: unknown): boolean => value instanceof Date;
export const isArgumentsObject = (value: unknown): boolean => tag(value) === "[object Arguments]";
export const isBigIntObject = (value: unknown): boolean => tag(value) === "[object BigInt]";
export const isBooleanObject = (value: unknown): boolean => tag(value) === "[object Boolean]";
export const isNumberObject = (value: unknown): boolean => tag(value) === "[object Number]";
export const isStringObject = (value: unknown): boolean => tag(value) === "[object String]";
export const isSymbolObject = (value: unknown): boolean => tag(value) === "[object Symbol]";
export const isNativeError = (value: unknown): boolean => value instanceof Error;
export const isRegExp = (value: unknown): boolean => value instanceof RegExp;
export const isAsyncFunction = (value: unknown): boolean => tag(value) === "[object AsyncFunction]";
export const isGeneratorFunction = (value: unknown): boolean => tag(value) === "[object GeneratorFunction]";
export const isGeneratorObject = (value: unknown): boolean => tag(value) === "[object Generator]";
export const isPromise = (value: unknown): boolean => value instanceof Promise;
export const isMap = (value: unknown): boolean => value instanceof Map;
export const isSet = (value: unknown): boolean => value instanceof Set;
export const isMapIterator = (value: unknown): boolean => tag(value) === "[object Map Iterator]";
export const isSetIterator = (value: unknown): boolean => tag(value) === "[object Set Iterator]";
export const isWeakMap = (value: unknown): boolean => value instanceof WeakMap;
export const isWeakSet = (value: unknown): boolean => value instanceof WeakSet;
export const isArrayBuffer = (value: unknown): boolean => value instanceof ArrayBuffer;
export const isDataView = (value: unknown): boolean => value instanceof DataView;
export const isSharedArrayBuffer = (value: unknown): boolean => (
  typeof SharedArrayBuffer !== "undefined" && value instanceof SharedArrayBuffer
);
export const isProxy = (): boolean => false;
export const isModuleNamespaceObject = (value: unknown): boolean => tag(value) === "[object Module]";
export const isAnyArrayBuffer = (value: unknown): boolean => isArrayBuffer(value) || isSharedArrayBuffer(value);
export const isBoxedPrimitive = (value: unknown): boolean => (
  isBigIntObject(value)
  || isBooleanObject(value)
  || isNumberObject(value)
  || isStringObject(value)
  || isSymbolObject(value)
);
export const isArrayBufferView = (value: unknown): boolean => ArrayBuffer.isView(value);
export { isTypedArray };
export const isUint8Array = (value: unknown): boolean => value instanceof Uint8Array;
export const isUint8ClampedArray = (value: unknown): boolean => value instanceof Uint8ClampedArray;
export const isUint16Array = (value: unknown): boolean => value instanceof Uint16Array;
export const isUint32Array = (value: unknown): boolean => value instanceof Uint32Array;
export const isInt8Array = (value: unknown): boolean => value instanceof Int8Array;
export const isInt16Array = (value: unknown): boolean => value instanceof Int16Array;
export const isInt32Array = (value: unknown): boolean => value instanceof Int32Array;
export const isFloat32Array = (value: unknown): boolean => value instanceof Float32Array;
export const isFloat64Array = (value: unknown): boolean => value instanceof Float64Array;
export const isBigInt64Array = (value: unknown): boolean => (
  typeof BigInt64Array !== "undefined" && value instanceof BigInt64Array
);
export const isBigUint64Array = (value: unknown): boolean => (
  typeof BigUint64Array !== "undefined" && value instanceof BigUint64Array
);
export const isKeyObject = (): boolean => false;
export const isCryptoKey = (value: unknown): boolean => (
  typeof CryptoKey !== "undefined" && value instanceof CryptoKey
);

const types = {
  isAnyArrayBuffer,
  isArgumentsObject,
  isArrayBuffer,
  isArrayBufferView,
  isAsyncFunction,
  isBigInt64Array,
  isBigIntObject,
  isBigUint64Array,
  isBooleanObject,
  isBoxedPrimitive,
  isCryptoKey,
  isDataView,
  isDate,
  isExternal,
  isFloat32Array,
  isFloat64Array,
  isGeneratorFunction,
  isGeneratorObject,
  isInt16Array,
  isInt32Array,
  isInt8Array,
  isKeyObject,
  isMap,
  isMapIterator,
  isModuleNamespaceObject,
  isNativeError,
  isNumberObject,
  isPromise,
  isProxy,
  isRegExp,
  isSet,
  isSetIterator,
  isSharedArrayBuffer,
  isStringObject,
  isSymbolObject,
  isTypedArray,
  isUint16Array,
  isUint32Array,
  isUint8Array,
  isUint8ClampedArray,
  isWeakMap,
  isWeakSet,
};

export default types;
