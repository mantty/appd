export function unsupportedMethod(module: string, method: string): (...arguments_: unknown[]) => never {
  return () => {
    throw new Error(`The ${module}.${method} method is not implemented`);
  };
}

export function unsupportedClass(module: string, name: string): new (...arguments_: unknown[]) => never {
  const Unsupported = function unsupported(): never {
    throw new Error(`The ${module}.${name} constructor is not implemented`);
  };
  return Unsupported as unknown as new (...arguments_: unknown[]) => never;
}
