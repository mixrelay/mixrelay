import { useCallback, useEffect, useRef, useState } from 'react';
import type { AudioGraphModel } from '../../core/audio-graph/model';
import type { ProcessorEditorStatus } from '../../core/domain/control/processors';
import { structuralShare } from '../../core/domain/structuralShare';
import type { ControlPort } from '../../core/ports/control/ControlPort';

export const EDITOR_POLL_MS = 250;
type EditorPort = Pick<ControlPort, 'getProcessorEditor' | 'setProcessorEditor' | 'subscribeState'>;
type Action = { kind: 'poll' } | { kind: 'open' };
export interface ProcessorEditorIssue {
  nodeId: string;
  name: string;
  busy: boolean;
}
interface Entry {
  id: string;
  name: string;
  sequence: number;
  command: boolean;
  /** Present a new node once; a user-closed native window stays closed across graph edits. */
  autoOpen: boolean;
  retry: Action;
  timer?: ReturnType<typeof setTimeout>;
}
interface Callbacks {
  removeStatus: (id: string) => void;
  invalidate: () => void;
  status: (status: ProcessorEditorStatus) => void;
  issue: (entry: Entry, busy: boolean | null) => void;
  busy: (entry: Entry) => void;
}
function identities(model: AudioGraphModel | null) {
  return (
    model?.nodes
      .filter(
        (node) =>
          node.runtime?.type === 'provider' &&
          node.runtime.control?.enabled !== false &&
          node.appearance.hasProcessorEditor,
      )
      .map((node) => {
        const config = node.runtime?.config as
          | { typeId?: string; ports?: { layoutId?: string } }
          | undefined;
        // A layout change replaces the native instance, so it is part of the identity.
        const identity = JSON.stringify([config?.typeId ?? '', config?.ports?.layoutId ?? '']);
        return [node.id, identity, node.appearance.title] as const;
      }) ?? []
  );
}

/** New nodes present their editor once. A node whose native instance is replaced (e.g. a
 * new channel layout) reopens only if its editor was open. The layout reaches the model
 * before the engine applies it, so until a newer document revision arrives the old, still
 * open instance may answer; the intent is kept (`reopen`, by revision of the change) and
 * spent on the first session at a later revision, which polls the replacement. */
function editorsToOpen(
  nodes: readonly (readonly [string, string, string])[],
  history: {
    kept: ReadonlyMap<string, ProcessorEditorStatus>;
    before: ReadonlyMap<string, ProcessorEditorStatus>;
    reopen: Map<string, number>;
    /** The previous session's document revision: a change seen without a newer one is
     * still optimistic. */
    priorRevision: number;
    revision: number;
  },
) {
  const { kept, before, reopen, priorRevision, revision } = history;
  const present = new Set(nodes.map(([id]) => id));
  for (const id of reopen.keys()) if (!present.has(id)) reopen.delete(id);
  const open = new Set<string>();
  for (const [id] of nodes) {
    const replaced = kept.has(id) ? undefined : before.get(id);
    if (replaced?.state === 'open') reopen.set(id, priorRevision);
    const changedAt = reopen.get(id);
    if (changedAt !== undefined && revision > changedAt) {
      reopen.delete(id);
      open.add(id);
    } else if (!kept.has(id) && !replaced) open.add(id);
  }
  return open;
}

/** Node/port lifetime and per-node request ordering stay independent of React render timing. */
class EditorSession {
  private cancelled = false;
  private pushed = false;
  private generation = 0;
  private latest?: ProcessorEditorStatus[];
  private unsubscribe?: () => void;
  private entries: Map<string, Entry>;
  constructor(
    private port: EditorPort,
    nodes: [string, string, string][],
    private callbacks: Callbacks,
    private expectedRevision: number,
    autoOpen: ReadonlySet<string>,
  ) {
    this.entries = new Map(
      nodes.map(([id, , name]) => [
        id,
        {
          id,
          name,
          sequence: 0,
          command: false,
          autoOpen: autoOpen.has(id),
          retry: { kind: 'poll' },
        },
      ]),
    );
  }
  start() {
    this.unsubscribe = this.port.subscribeState?.((state) => {
      this.generation += 1;
      this.pushed = Boolean(state);
      this.latest =
        state?.graph.submittedRevision === this.expectedRevision ? state.editors : undefined;
      if (!this.pushed) this.callbacks.invalidate();
      for (const entry of this.entries.values()) this.pushEntry(entry, this.latest);
    });
    for (const entry of this.entries.values()) void this.run(entry, { kind: 'poll' });
  }
  /** Applies pushed state, or without push forgets the stale status and polls instead. An
   * entry with a command in flight is applied when that command completes. */
  private pushEntry(entry: Entry, editors?: ProcessorEditorStatus[]) {
    if (entry.command) return;
    entry.sequence += 1;
    if (entry.timer) clearTimeout(entry.timer);
    const status = this.pushed ? editors?.find((editor) => editor.nodeId === entry.id) : undefined;
    if (status) return this.receive(entry, status, { kind: 'poll' });
    this.callbacks.removeStatus(entry.id);
    this.callbacks.issue(entry, false);
    if (!this.pushed)
      entry.timer = setTimeout(() => void this.run(entry, { kind: 'poll' }), EDITOR_POLL_MS);
  }
  private valid(entry: Entry, sequence: number) {
    return !this.cancelled && this.entries.get(entry.id) === entry && sequence === entry.sequence;
  }
  private receive(entry: Entry, status: ProcessorEditorStatus, action: Action) {
    if (status.nodeId !== entry.id) throw new Error('Mismatched editor response');
    this.callbacks.status(status);
    if (
      status.state === 'failed' ||
      status.state === 'unavailable' ||
      (status.state === 'closed' && action.kind === 'open')
    ) {
      entry.retry = { kind: 'open' };
      this.callbacks.issue(entry, false);
      return;
    }
    this.callbacks.issue(entry, null);
    if (status.state === 'closed') {
      entry.retry = { kind: 'open' };
      if (entry.autoOpen) {
        entry.autoOpen = false;
        void this.run(entry, { kind: 'open' });
      }
      return;
    }
    // Failed, unavailable and closed states returned above; open/ opening keep polling.
    entry.autoOpen = false;
    if (!this.pushed)
      entry.timer = setTimeout(() => {
        void this.run(entry, { kind: 'poll' });
      }, EDITOR_POLL_MS);
  }
  private async run(entry: Entry, action: Action) {
    if (this.cancelled || entry.command || (action.kind === 'poll' && this.pushed)) return;
    if (entry.timer) clearTimeout(entry.timer);
    const sequence = ++entry.sequence;
    const generation = this.generation;
    entry.command = action.kind === 'open';
    if (action.kind === 'open') entry.retry = action;
    this.callbacks.busy(entry);
    try {
      const status = await (action.kind === 'poll'
        ? this.port.getProcessorEditor(entry.id)
        : this.port.setProcessorEditor(entry.id, true, this.expectedRevision));
      if (!this.valid(entry, sequence)) return;
      entry.command = false;
      if (generation !== this.generation) this.pushEntry(entry, this.latest);
      else this.receive(entry, status, action);
    } catch {
      if (!this.valid(entry, sequence)) return;
      entry.command = false;
      if (generation !== this.generation) {
        this.pushEntry(entry, this.latest);
        return;
      }
      entry.retry = action;
      this.callbacks.issue(entry, false); // Keep the last confirmed native state.
    }
  }
  retry(id: string) {
    const entry = this.entries.get(id);
    if (entry) void this.run(entry, entry.retry);
  }
  dispose() {
    this.cancelled = true;
    this.unsubscribe?.();
    for (const entry of this.entries.values()) if (entry.timer) clearTimeout(entry.timer);
    // Native editor ownership belongs to the processor runtime. The control API resolves
    // IDs in the selected graph, so a late close here could close a different graph's node.
  }
}

export function useProcessorEditors(
  controlPort: EditorPort,
  model: AudioGraphModel | null,
  graphId?: string,
) {
  const [statuses, setStatuses] = useState<Map<string, ProcessorEditorStatus>>(() => new Map());
  const [issues, setIssues] = useState<Map<string, ProcessorEditorIssue>>(() => new Map());
  const statusesRef = useRef(statuses);
  statusesRef.current = statuses;
  const sessionRef = useRef<EditorSession | null>(null);
  const reopenRef = useRef(new Map<string, number>());
  const previous = useRef<{
    port: EditorPort;
    keys: Map<string, string>;
    graphId?: string;
    revision: number;
  } | null>(null);
  const key = JSON.stringify([model?.revision ?? 0, identities(model)]);
  useEffect(() => {
    const [expectedRevision, nodes]: [number, [string, string, string][]] = JSON.parse(key);
    const keys = new Map(nodes.map(([id, type]) => [id, type]));
    const prior = previous.current;
    const before = statusesRef.current;
    const samePlace = prior?.port === controlPort && prior.graphId === graphId;
    // Intents belong to one graph on one port; node IDs repeat across graphs.
    if (!samePlace) reopenRef.current.clear();
    statusesRef.current = new Map(
      [...before].filter(
        ([id]) => samePlace && keys.has(id) && prior?.keys.get(id) === keys.get(id),
      ),
    );
    const autoOpen = editorsToOpen(nodes, {
      kept: statusesRef.current,
      before: samePlace ? before : new Map(),
      reopen: reopenRef.current,
      // Used only for replaced nodes, which exist only when a prior session does.
      priorRevision: prior?.revision ?? expectedRevision,
      revision: expectedRevision,
    });
    setStatuses(statusesRef.current);
    previous.current = { port: controlPort, keys, graphId, revision: expectedRevision };
    setIssues(new Map());
    const session = new EditorSession(
      controlPort,
      nodes,
      {
        removeStatus: (id) => {
          if (!statusesRef.current.has(id)) return;
          statusesRef.current = new Map(statusesRef.current);
          statusesRef.current.delete(id);
          setStatuses(statusesRef.current);
        },
        invalidate: () => {
          statusesRef.current = new Map();
          setStatuses(new Map());
        },
        status: (status) => {
          // Polled every few hundred ms: an unchanged status must not re-render the graph.
          const previous = statusesRef.current.get(status.nodeId);
          if (structuralShare(previous, status) === previous) return;
          statusesRef.current = new Map(statusesRef.current).set(status.nodeId, status);
          setStatuses(statusesRef.current);
        },
        issue: (entry, busy) =>
          setIssues((current) => {
            const existing = current.get(entry.id);
            // Every poll reports its issue state; an unchanged one must not re-render.
            // An entry's name is fixed for its lifetime; the session resets the map.
            if (busy === null ? !existing : existing?.busy === busy) return current;
            const next = new Map(current);
            if (busy === null) next.delete(entry.id);
            else next.set(entry.id, { nodeId: entry.id, name: entry.name, busy });
            return next;
          }),
        busy: (entry) =>
          setIssues((current) =>
            current.has(entry.id)
              ? new Map(current).set(entry.id, { nodeId: entry.id, name: entry.name, busy: true })
              : current,
          ),
      },
      expectedRevision,
      autoOpen,
    );
    sessionRef.current = session;
    session.start();
    return () => {
      session.dispose();
      sessionRef.current = null;
    };
  }, [controlPort, key, graphId]);
  const retry = useCallback((id: string) => sessionRef.current?.retry(id), []);
  return { statuses, issues, retry };
}
