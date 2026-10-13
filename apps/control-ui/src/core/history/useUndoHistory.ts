import { useCallback, useEffect, useRef } from 'react';

const historyLimit = 100;
const mergeWindowMs = 600;

interface HistoryEntry<Value, Kind> {
  after: Value;
  before: Value;
  changedAt: number;
  group?: string;
  kind: Kind;
}

export interface HistoryStep<Value, Kind> {
  kind: Kind;
  value: Value;
}

/** Adjacent entries share snapshots (one's `after` is the next one's `before`). Each
 * distinct snapshot is transformed once, so the rebased history keeps sharing them. */
function rebaseEntries<Value, Kind>(
  entries: HistoryEntry<Value, Kind>[],
  transform: (value: Value) => Value,
) {
  const rebased = new Map<Value, Value>();
  const once = (value: Value) => {
    if (!rebased.has(value)) rebased.set(value, transform(value));
    return rebased.get(value) as Value;
  };
  for (const entry of entries) {
    entry.before = once(entry.before);
    entry.after = once(entry.after);
  }
}

export function useUndoHistory<Value, Kind>() {
  const undoStack = useRef<HistoryEntry<Value, Kind>[]>([]);
  const redoStack = useRef<HistoryEntry<Value, Kind>[]>([]);
  const navigatedSinceRecord = useRef(false);
  const pendingEntry = useRef<HistoryEntry<Value, Kind> | null>(null);
  const pendingTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const cancelPendingTimer = useCallback(() => {
    if (pendingTimer.current) clearTimeout(pendingTimer.current);
    pendingTimer.current = null;
  }, []);

  const commitEntry = useCallback((entry: HistoryEntry<Value, Kind>) => {
    undoStack.current.push(entry);
    if (undoStack.current.length > historyLimit) undoStack.current.shift();
    redoStack.current = [];
    navigatedSinceRecord.current = false;
  }, []);

  const flush = useCallback(() => {
    cancelPendingTimer();
    const pending = pendingEntry.current;
    pendingEntry.current = null;
    if (pending) commitEntry(pending);
  }, [cancelPendingTimer, commitEntry]);

  const clear = useCallback(() => {
    cancelPendingTimer();
    pendingEntry.current = null;
    undoStack.current = [];
    redoStack.current = [];
    navigatedSinceRecord.current = false;
  }, [cancelPendingTimer]);

  const record = useCallback(
    (before: Value, after: Value, kind: Kind, group?: string) => {
      // A structural edit must follow (not precede) the live control gesture awaiting commit.
      flush();
      const changedAt = Date.now();
      const previous = undoStack.current.at(-1);
      const merge =
        !navigatedSinceRecord.current &&
        group !== undefined &&
        previous?.group === group &&
        previous.kind === kind &&
        changedAt - previous.changedAt <= mergeWindowMs;
      if (merge && previous) {
        previous.after = after;
        previous.changedAt = changedAt;
        previous.kind = kind;
      } else {
        undoStack.current.push({ after, before, changedAt, group, kind });
        if (undoStack.current.length > historyLimit) undoStack.current.shift();
      }
      redoStack.current = [];
      navigatedSinceRecord.current = false;
    },
    [flush],
  );

  const recordDebounced = useCallback(
    (before: Value, after: Value, kind: Kind, group: string, delayMs = 400) => {
      const previous = pendingEntry.current;
      if (previous && (previous.group !== group || previous.kind !== kind)) flush();
      const pending = pendingEntry.current;
      pendingEntry.current = pending
        ? { ...pending, after, changedAt: Date.now(), kind }
        : { after, before, changedAt: Date.now(), group, kind };
      cancelPendingTimer();
      pendingTimer.current = setTimeout(flush, delayMs);
    },
    [cancelPendingTimer, flush],
  );

  const undo = useCallback((): HistoryStep<Value, Kind> | null => {
    flush();
    const entry = undoStack.current.pop();
    if (!entry) return null;
    redoStack.current.push(entry);
    navigatedSinceRecord.current = true;
    return { kind: entry.kind, value: entry.before };
  }, [flush]);

  const redo = useCallback((): HistoryStep<Value, Kind> | null => {
    flush();
    const entry = redoStack.current.pop();
    if (!entry) return null;
    undoStack.current.push(entry);
    navigatedSinceRecord.current = true;
    return { kind: entry.kind, value: entry.after };
  }, [flush]);

  const rebase = useCallback((transform: (value: Value) => Value) => {
    const entries = [...undoStack.current, ...redoStack.current];
    if (pendingEntry.current) entries.push(pendingEntry.current);
    rebaseEntries(entries, transform);
  }, []);

  useEffect(() => () => cancelPendingTimer(), [cancelPendingTimer]);

  return { clear, flush, record, recordDebounced, redo, undo, rebase };
}
