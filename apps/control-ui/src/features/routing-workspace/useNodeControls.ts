import { useEffect, useRef, useState } from 'react';
import type { ControlPort, GraphNodeControls } from '../../core/control-port/types';
import type { GraphApplicationQueue } from './GraphApplicationQueue';

/** A single bounded-rate reader; stale graph/port replies cannot affect a replacement workspace. */
export function useNodeControls(
  port: ControlPort,
  graphId: string | undefined,
  revision: number,
  idle: boolean,
  application: GraphApplicationQueue,
) {
  const [snapshot, setSnapshot] = useState<GraphNodeControls | null>(null);
  const latest = useRef({ revision, idle });
  latest.current = { revision, idle };
  useEffect(() => {
    setSnapshot(null);
    if (!graphId || !port.getNodeControls) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const canRead = () => !cancelled && latest.current.idle && application.canPublishParameters;
    const receive = (result: GraphNodeControls | undefined) => {
      if (!canRead() || result?.graphId !== graphId) return;
      setSnapshot(result);
      if (result.revision > latest.current.revision) application.refresh();
    };
    const poll = async () => {
      try {
        if (canRead()) receive(await port.getNodeControls?.(graphId));
      } catch {
        // An unreachable controller must not leave apparently actionable stale buttons.
        if (!cancelled) setSnapshot(null);
      } finally {
        if (!cancelled) timer = setTimeout(() => void poll(), 1000);
      }
    };
    void poll();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [port, graphId, application]);
  return snapshot;
}
