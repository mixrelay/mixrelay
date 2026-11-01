/** Cancels a scheduled task; calling it after the task ran or twice does nothing. */
export type Cancel = () => void;

/** Runs a task later. Code that waits takes this port so tests control time explicitly. */
export interface Scheduler {
  after(delayMs: number, task: () => void): Cancel;
}

/** Wall-clock milliseconds, as compared between successive readings. */
export interface Clock {
  now(): number;
}
