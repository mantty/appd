import { unsupportedMethod } from "./not-implemented.js";

interface MockOptions {
  readonly times?: number;
}

interface MockCall {
  arguments: unknown[];
  error: unknown;
  result: unknown;
  stack: Error;
  target: Function | undefined;
  this: unknown;
}

interface MockContext {
  readonly calls: MockCall[];
  callCount(): number;
  mockImplementation(implementation: Function): void;
  mockImplementationOnce(implementation: Function, onCall?: number): void;
  resetCalls(): void;
  restore(): void;
}

type MockedFunction = Function & { readonly mock: MockContext };

const mockRestorers = new WeakMap<MockedFunction, (restore: () => void) => void>();

export class MockTracker {
  readonly #restore = new Set<() => void>();

  fn(
    original?: Function | MockOptions,
    implementation?: Function | MockOptions,
    options?: MockOptions,
  ): MockedFunction {
    if (isMockOptions(original)) return this.create(undefined, undefined, original);
    const configured = typeof implementation === "function" ? implementation : undefined;
    const configuration = typeof implementation === "function" ? options : implementation;
    return this.create(original, configured, configuration);
  }

  method(object: object, name: PropertyKey, implementation?: Function | MockOptions, options?: MockOptions): MockedFunction {
    const descriptor = propertyDescriptor(object, name);
    if (typeof descriptor?.value !== "function") throw new TypeError("The property is not a method");
    const mocked = this.fn(descriptor.value, implementation, options);
    this.replace(object, name, { ...descriptor, value: mocked });
    return mocked;
  }

  getter(object: object, name: PropertyKey, implementation?: Function | MockOptions, options?: MockOptions): MockedFunction {
    const descriptor = propertyDescriptor(object, name);
    if (typeof descriptor?.get !== "function") throw new TypeError("The property is not a getter");
    const mocked = this.fn(descriptor.get, implementation, options);
    this.replace(object, name, { ...descriptor, get: mocked as unknown as () => unknown });
    return mocked;
  }

  setter(object: object, name: PropertyKey, implementation?: Function | MockOptions, options?: MockOptions): MockedFunction {
    const descriptor = propertyDescriptor(object, name);
    if (typeof descriptor?.set !== "function") throw new TypeError("The property is not a setter");
    const mocked = this.fn(descriptor.set, implementation, options);
    this.replace(object, name, { ...descriptor, set: mocked as unknown as (value: unknown) => void });
    return mocked;
  }

  reset(): void {
    this.restoreAll();
    this.#restore.clear();
  }

  restoreAll(): void {
    for (const restore of this.#restore) restore();
  }

  readonly timers = unsupportedMethod("test.mock", "timers");

  private create(original: Function | undefined, initial: Function | undefined, options: MockOptions | undefined): MockedFunction {
    const fallback = original ?? function (): undefined { return undefined; };
    const calls: MockCall[] = [];
    const once = new Map<number, Function>();
    let implementation = initial;
    let active = true;
    const times = validatedTimes(options);
    let invocation = 0;
    let restore = (): void => { active = false; };

    const mocked = function mock(this: unknown, ...arguments_: unknown[]): unknown {
      const target = new.target;
      const current = once.get(invocation) ?? (active && invocation < times ? implementation : undefined) ?? fallback;
      once.delete(invocation);
      invocation += 1;
      const call: MockCall = {
        arguments: arguments_,
        error: undefined,
        result: undefined,
        stack: new Error(),
        target,
        this: this,
      };
      try {
        call.result = target === undefined
          ? Reflect.apply(current, this, arguments_)
          : Reflect.construct(
            current,
            arguments_,
            target === (mocked as unknown as Function) && original !== undefined ? original : target,
          );
        return call.result;
      } catch (error) {
        call.error = error;
        throw error;
      } finally {
        calls.push(call);
      }
    } as unknown as MockedFunction;

    const context: MockContext = {
      get calls(): MockCall[] { return [...calls]; },
      callCount: (): number => calls.length,
      mockImplementation: (next: Function): void => { implementation = next; },
      mockImplementationOnce: (next: Function, onCall?: number): void => {
        const call = onCall ?? invocation;
        validateCallIndex(call, "onCall", invocation);
        once.set(call, next);
      },
      resetCalls: (): void => { calls.length = 0; },
      restore: (): void => { restore(); },
    };
    Object.defineProperty(mocked, "mock", { value: context });
    mockRestorers.set(mocked, (replacement) => {
      const previous = restore;
      restore = (): void => {
        previous();
        replacement();
      };
    });
    this.#restore.add(context.restore);
    return mocked;
  }

  private replace(object: object, name: PropertyKey, descriptor: PropertyDescriptor): void {
    const own = Object.getOwnPropertyDescriptor(object, name);
    Object.defineProperty(object, name, descriptor);
    const restore = () => {
      if (own === undefined) Reflect.deleteProperty(object, name);
      else Object.defineProperty(object, name, own);
    };
    const mocked = descriptor.value ?? descriptor.get ?? descriptor.set;
    mockRestorers.get(mocked as MockedFunction)?.(restore);
  }
}

function isMockOptions(value: unknown): value is MockOptions {
  return typeof value === "object" && value !== null;
}

function validatedTimes(options: MockOptions | undefined): number {
  if (options?.times === undefined) return Infinity;
  validateCallIndex(options.times, "options.times", 1);
  return options.times;
}

function validateCallIndex(value: unknown, name: string, minimum: number): asserts value is number {
  if (typeof value !== "number") throw new TypeError(`The ${name} value must be a number`);
  if (!Number.isInteger(value)) throw new RangeError(`The ${name} value must be an integer`);
  if (value < minimum || value > Number.MAX_SAFE_INTEGER) {
    throw new RangeError(`The ${name} value is out of range`);
  }
}

function propertyDescriptor(object: object, name: PropertyKey): PropertyDescriptor | undefined {
  let current: object | null = object;
  while (current !== null) {
    const descriptor = Object.getOwnPropertyDescriptor(current, name);
    if (descriptor !== undefined) return descriptor;
    current = Object.getPrototypeOf(current);
  }
  return undefined;
}

export const mock = new MockTracker();
export const run = unsupportedMethod("test", "run");
export const test = unsupportedMethod("test", "test");
export const describe = test;
export const it = test;
export const only = test;
export const skip = test;
export const todo = test;

export default test;
