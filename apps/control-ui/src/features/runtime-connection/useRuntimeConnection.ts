import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  RuntimeConnectStage,
  RuntimePort,
  RuntimeStatus,
} from '../../core/runtime/RuntimePort';

type ConnectionState = RuntimeStatus | 'connecting';

export function useRuntimeConnection(port: RuntimePort) {
  const [state, setState] = useState<ConnectionState>('connecting');
  const [stage, setStage] = useState<RuntimeConnectStage>('checking');
  const [opened, setOpened] = useState(false);
  const generation = useRef(0);
  const connect = useCallback(async () => {
    const request = ++generation.current;
    setState('connecting');
    setStage('checking');
    let result: RuntimeStatus;
    try {
      result = await port.connect((next) => {
        if (generation.current === request) setStage(next);
      });
    } catch {
      result = 'unavailable';
    }
    if (generation.current !== request) return;
    if (result === 'ready') setOpened(true);
    setState(result);
  }, [port]);

  useEffect(() => {
    void connect();
    return () => {
      generation.current += 1;
    };
  }, [connect]);

  useEffect(() => {
    if (state !== 'ready' && !(opened && state === 'unavailable')) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const check = async () => {
      let result: RuntimeStatus;
      try {
        result = await port.status();
      } catch {
        result = 'unavailable';
      }
      if (cancelled) return;
      setState(result);
      if (result === 'ready' || result === 'unavailable')
        timer = setTimeout(() => void check(), 2000);
    };
    timer = setTimeout(() => void check(), 2000);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [port, state, opened]);

  return { state, stage, opened, connect };
}
