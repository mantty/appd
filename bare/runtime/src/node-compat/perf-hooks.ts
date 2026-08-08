import { unsupportedClass, unsupportedMethod } from "./not-implemented.js";
import { performance as runtimePerformance } from "../performance.js";

export const performance = runtimePerformance;
export const Performance = unsupportedClass("perf_hooks", "Performance");
export const PerformanceEntry = unsupportedClass("perf_hooks", "PerformanceEntry");
export const PerformanceMark = unsupportedClass("perf_hooks", "PerformanceMark");
export const PerformanceMeasure = unsupportedClass("perf_hooks", "PerformanceMeasure");
export const PerformanceObserver = unsupportedClass("perf_hooks", "PerformanceObserver");
export const PerformanceObserverEntryList = unsupportedClass("perf_hooks", "PerformanceObserverEntryList");
export const PerformanceResourceTiming = unsupportedClass("perf_hooks", "PerformanceResourceTiming");
export const constants = Object.freeze({});
export const createHistogram = unsupportedMethod("perf_hooks", "createHistogram");
export const monitorEventLoopDelay = unsupportedMethod("perf_hooks", "monitorEventLoopDelay");

export default {
  Performance,
  PerformanceEntry,
  PerformanceMark,
  PerformanceMeasure,
  PerformanceObserver,
  PerformanceObserverEntryList,
  PerformanceResourceTiming,
  constants,
  createHistogram,
  monitorEventLoopDelay,
  performance,
};
