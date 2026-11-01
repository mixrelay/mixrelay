import { useEffect, useRef } from 'react';
import type { GraphNodeControls } from '../../core/domain/control/graph';
import { graphControls } from '../../core/domain/control/state';
import type { ControlPort } from '../../core/ports/control/ControlPort';
import { useSharedState } from '../../core/structuralShare';
import type { GraphApplicationQueue } from './GraphApplicationQueue';

/** A single bounded-rate reader; stale graph/port replies cannot affect a replacement workspace. */
export function useNodeControls(
  port: ControlPort,
  graphId: string | undefined,
  revision: number,
  idle: boolean,
  application: GraphApplicationQueue,
) {
  const [snapshot, setSnapshot] = useSharedState<GraphNodeControls | null>(null);
  const latest = useRef({ revision, idle });
  latest.current = { revision, idle };
  useEffect(() => {
    setSnapshot(null);
    if (!graphId || !port.getNodeControls) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    let incarnation: string | undefined;
    let resync = false;
    const canRead = () => !cancelled && latest.current.idle && application.canPublishParameters;
    const receive = (result: GraphNodeControls | undefined) => {
      if (!canRead()) return;
      if (resync) {
        resync = false;
        application.refresh();
      }
      if (result?.graphId !== graphId) return;
      setSnapshot(result);
      if (result.revision > latest.current.revision) application.refresh();
    };
    let pushed = false;
    let committed: GraphNodeControls | undefined;
    let generation = 0;
    const unsubscribe = port.subscribeState?.((state, engine) => {
      if (state && engine) {
        resync ||= incarnation !== undefined && incarnation !== engine;
        incarnation = engine;
      }
      generation += 1;
      pushed = Boolean(state);
      committed = graphControls(state, graphId);
      receive(committed);
      if (!committed) setSnapshot(null);
    });
    const schedule = () => {
      if (!cancelled) timer = setTimeout(() => void poll(), 1000);
    };
    const ready = () => {
      if (pushed) receive(committed);
      return !pushed && canRead();
    };
    const poll = async () => {
      const attempt = generation;
      const current = () => !cancelled && attempt === generation;
      if (!ready()) {
        schedule();
        return;
      }
      try {
        const result = await port.getNodeControls?.(graphId);
        if (current()) receive(result);
      } catch {
        // An unreachable controller must not leave apparently actionable stale buttons.
        if (current()) setSnapshot(null);
      } finally {
        schedule();
      }
    };
    void poll();
    return () => {
      cancelled = true;
      unsubscribe?.();
      clearTimeout(timer);
    };
  }, [port, graphId, application, setSnapshot]);
  return snapshot;
}
