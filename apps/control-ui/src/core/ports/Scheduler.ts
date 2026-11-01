/** Cancels a scheduled task; calling it after the task ran or twice does nothing. */
export type Cancel = () => void;

/** Runs a task later. The telemetry hub, the HTTP and Tauri control adapters, the pushed-revision
 * wait and undo history take it; feature and Core UI code still call the browser timers directly. */
export interface Scheduler {
  after(delayMs: number, task: () => void): Cancel;
}

/** Wall-clock milliseconds, as compared between successive readings. Only undo history reads it. */
export interface Clock {
  now(): number;
}
