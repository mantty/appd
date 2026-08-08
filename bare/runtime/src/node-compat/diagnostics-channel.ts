import DiagnosticsChannel from "bare-diagnostics-channel";

export const {
  Channel,
} = DiagnosticsChannel;
export const channel = (name: string) => DiagnosticsChannel.channel(name);
export const hasSubscribers = (name: string) => DiagnosticsChannel.hasSubscribers(name);
export const subscribe = (name: string, listener: (message: unknown, channel: string) => void) => {
  DiagnosticsChannel.subscribe(name, listener);
};
export const unsubscribe = (name: string, listener: (message: unknown, channel: string) => void) => {
  return DiagnosticsChannel.unsubscribe(name, listener);
};

interface TraceContext {
  error?: unknown;
  result?: unknown;
}

type TraceEvent = "start" | "end" | "asyncStart" | "asyncEnd" | "error";
type TraceSubscriber = (context: TraceContext) => void;

interface TraceChannel {
  publish(context: TraceContext): void;
  subscribe(listener: TraceSubscriber): void;
  unsubscribe(listener: TraceSubscriber): boolean;
}

class TracingChannel {
  readonly start: TraceChannel;
  readonly end: TraceChannel;
  readonly asyncStart: TraceChannel;
  readonly asyncEnd: TraceChannel;
  readonly error: TraceChannel;
  private readonly names: Readonly<Record<TraceEvent, string>>;

  constructor(name: string) {
    this.names = {
      asyncEnd: `tracing:${name}:asyncEnd`,
      asyncStart: `tracing:${name}:asyncStart`,
      end: `tracing:${name}:end`,
      error: `tracing:${name}:error`,
      start: `tracing:${name}:start`,
    };
    this.start = channel(this.names.start) as TraceChannel;
    this.end = channel(this.names.end) as TraceChannel;
    this.asyncStart = channel(this.names.asyncStart) as TraceChannel;
    this.asyncEnd = channel(this.names.asyncEnd) as TraceChannel;
    this.error = channel(this.names.error) as TraceChannel;
  }

  get hasSubscribers(): boolean {
    return Object.values(this.names).some((name) => hasSubscribers(name));
  }

  subscribe(subscribers: Partial<Record<TraceEvent, TraceSubscriber>>): void {
    for (const event of traceEvents) {
      const subscriber = subscribers[event];
      if (subscriber !== undefined) this[event].subscribe(subscriber);
    }
  }

  unsubscribe(subscribers: Partial<Record<TraceEvent, TraceSubscriber>>): boolean {
    let removed = true;
    for (const event of traceEvents) {
      const subscriber = subscribers[event];
      if (subscriber !== undefined) removed = this[event].unsubscribe(subscriber) && removed;
    }
    return removed;
  }

  traceSync<Result>(
    function_: (this: unknown, ...arguments_: unknown[]) => Result,
    context: TraceContext = {},
    thisArg?: unknown,
    ...arguments_: unknown[]
  ): Result {
    this.start.publish(context);
    try {
      const result = Reflect.apply(function_, thisArg, arguments_);
      context.result = result;
      return result;
    } catch (error: unknown) {
      context.error = error;
      this.error.publish(context);
      throw error;
    } finally {
      this.end.publish(context);
    }
  }

  tracePromise<Result>(
    function_: (this: unknown, ...arguments_: unknown[]) => Promise<Result>,
    context: TraceContext = {},
    thisArg?: unknown,
    ...arguments_: unknown[]
  ): Promise<Result> {
    this.start.publish(context);
    try {
      return Promise.resolve(Reflect.apply(function_, thisArg, arguments_)).then(
        (result) => {
          context.result = result;
          this.asyncStart.publish(context);
          this.asyncEnd.publish(context);
          return result;
        },
        (error: unknown) => {
          context.error = error;
          this.error.publish(context);
          this.asyncStart.publish(context);
          this.asyncEnd.publish(context);
          throw error;
        },
      );
    } catch (error: unknown) {
      context.error = error;
      this.error.publish(context);
      throw error;
    } finally {
      this.end.publish(context);
    }
  }

  traceCallback<Result>(
    function_: (this: unknown, ...arguments_: unknown[]) => Result,
    position = -1,
    context: TraceContext = {},
    thisArg?: unknown,
    ...arguments_: unknown[]
  ): Result {
    const callbackIndex = position < 0 ? arguments_.length + position : position;
    const callback = arguments_[callbackIndex];
    if (typeof callback !== "function") throw new TypeError("The callback argument must be a function");

    arguments_[callbackIndex] = (...callbackArguments: unknown[]): unknown => {
      const [error, result] = callbackArguments;
      if (error === undefined || error === null) context.result = result;
      else {
        context.error = error;
        this.error.publish(context);
      }
      this.asyncStart.publish(context);
      try {
        return callback(...callbackArguments);
      } finally {
        this.asyncEnd.publish(context);
      }
    };

    return this.traceSync(function_, context, thisArg, ...arguments_);
  }
}

const traceEvents: readonly TraceEvent[] = ["start", "end", "asyncStart", "asyncEnd", "error"];
export const tracingChannel = (name: string) => new TracingChannel(name);

export default { Channel, channel, hasSubscribers, subscribe, tracingChannel, unsubscribe };
