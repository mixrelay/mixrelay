import { useCallback, useEffect, useRef, useState } from 'react';

import type { AudioGraphModel } from '../../core/audio-graph/model';
import type { ControlPort, ProcessorEditorStatus } from '../../core/control-port/types';

export const EDITOR_POLL_MS = 250;
type EditorPort = Pick<ControlPort, 'getProcessorEditor' | 'setProcessorEditor'>;
type Action = { kind: 'poll' } | { kind: 'toggle'; open: boolean };
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
  retry: Action;
  timer?: ReturnType<typeof setTimeout>;
}
interface Callbacks {
  read: (id: string) => ProcessorEditorStatus | undefined;
  status: (status: ProcessorEditorStatus) => void;
  issue: (entry: Entry, busy: boolean | null) => void;
  busy: (entry: Entry) => void;
}
function identities(model: AudioGraphModel | null) {
  return (
    model?.nodes
      .filter((node) => node.runtime?.type === 'provider' && node.appearance.hasProcessorEditor)
      .map((node) => {
        const config = node.runtime?.config as { typeId?: string } | undefined;
        return [node.id, config?.typeId ?? '', node.appearance.title] as const;
      }) ?? []
  );
}

/** Node/port lifetime and per-node request ordering stay independent of React render timing. */
class EditorSession {
  private cancelled = false;
  private entries: Map<string, Entry>;
  constructor(
    private port: EditorPort,
    nodes: [string, string, string][],
    private callbacks: Callbacks,
  ) {
    this.entries = new Map(
      nodes.map(([id, , name]) => [
        id,
        {
          id,
          name,
          sequence: 0,
          command: false,
          retry: { kind: 'poll' },
        },
      ]),
    );
  }
  start() {
    for (const entry of this.entries.values()) void this.run(entry, { kind: 'poll' });
  }
  private valid(entry: Entry, sequence: number) {
    return !this.cancelled && this.entries.get(entry.id) === entry && sequence === entry.sequence;
  }
  private receive(entry: Entry, status: ProcessorEditorStatus) {
    if (status.nodeId !== entry.id) throw new Error('Mismatched editor response');
    this.callbacks.status(status);
    if (status.state === 'failed' || status.state === 'unavailable') {
      if (entry.retry.kind === 'poll') entry.retry = { kind: 'toggle', open: true };
      this.callbacks.issue(entry, false);
      return;
    }
    this.callbacks.issue(entry, null);
    if (status.state === 'open' || status.state === 'opening') {
      entry.timer = setTimeout(() => {
        void this.run(entry, { kind: 'poll' });
      }, EDITOR_POLL_MS);
    }
  }
  private async run(entry: Entry, action: Action) {
    if (this.cancelled || entry.command) return;
    if (entry.timer) clearTimeout(entry.timer);
    const sequence = ++entry.sequence;
    entry.command = action.kind === 'toggle';
    if (action.kind === 'toggle') entry.retry = action;
    this.callbacks.busy(entry);
    try {
      const status = await (action.kind === 'poll'
        ? this.port.getProcessorEditor(entry.id)
        : this.port.setProcessorEditor(entry.id, action.open));
      if (!this.valid(entry, sequence)) return;
      entry.command = false;
      this.receive(entry, status);
    } catch {
      if (!this.valid(entry, sequence)) return;
      entry.command = false;
      entry.retry = action;
      this.callbacks.issue(entry, false); // Keep the last confirmed native state.
    }
  }
  toggle(id: string) {
    const entry = this.entries.get(id);
    if (!entry) return;
    const state = this.callbacks.read(id)?.state;
    void this.run(entry, { kind: 'toggle', open: state !== 'open' && state !== 'opening' });
  }
  retry(id: string) {
    const entry = this.entries.get(id);
    if (entry) void this.run(entry, entry.retry);
  }
  dispose() {
    this.cancelled = true;
    for (const entry of this.entries.values()) if (entry.timer) clearTimeout(entry.timer);
  }
}

export function useProcessorEditors(controlPort: EditorPort, model: AudioGraphModel | null) {
  const [statuses, setStatuses] = useState<Map<string, ProcessorEditorStatus>>(() => new Map());
  const [issues, setIssues] = useState<Map<string, ProcessorEditorIssue>>(() => new Map());
  const statusesRef = useRef(statuses);
  statusesRef.current = statuses;
  const sessionRef = useRef<EditorSession | null>(null);
  const previous = useRef<{ port: EditorPort; keys: Map<string, string> } | null>(null);
  const key = JSON.stringify(identities(model));
  useEffect(() => {
    const nodes: [string, string, string][] = JSON.parse(key);
    const keys = new Map(nodes.map(([id, type]) => [id, type]));
    const prior = previous.current;
    statusesRef.current = new Map(
      [...statusesRef.current].filter(
        ([id]) =>
          prior?.port === controlPort && keys.has(id) && prior.keys.get(id) === keys.get(id),
      ),
    );
    setStatuses(statusesRef.current);
    previous.current = { port: controlPort, keys };
    setIssues(new Map());
    const session = new EditorSession(controlPort, nodes, {
      read: (id) => statusesRef.current.get(id),
      status: (status) => {
        statusesRef.current = new Map(statusesRef.current).set(status.nodeId, status);
        setStatuses(statusesRef.current);
      },
      issue: (entry, busy) =>
        setIssues((current) => {
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
    });
    sessionRef.current = session;
    session.start();
    return () => {
      session.dispose();
      sessionRef.current = null;
    };
  }, [controlPort, key]);
  const toggle = useCallback((id: string) => sessionRef.current?.toggle(id), []);
  const retry = useCallback((id: string) => sessionRef.current?.retry(id), []);
  return { statuses, issues, toggle, retry };
}
