const origin = Date.now();
let last = 0;

export const performance = Object.freeze({
  now(): number {
    last = Math.max(last, Date.now() - origin);
    return last;
  },
  timeOrigin: origin,
  toJSON(): { readonly timeOrigin: number } {
    return { timeOrigin: origin };
  },
});
