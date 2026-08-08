import EventEmitter from "bare-events";

export const {
  defaultMaxListeners,
  errors,
  forward,
  getMaxListeners,
  listenerCount,
  on,
  once,
  setMaxListeners,
} = EventEmitter;
export { EventEmitter };

export default Object.assign(EventEmitter, {
  EventEmitter,
  defaultMaxListeners,
  errors,
  forward,
  getMaxListeners,
  listenerCount,
  on,
  once,
  setMaxListeners,
});
