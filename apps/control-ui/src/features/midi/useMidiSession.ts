import { useEffect, useRef, useState } from 'react';
import type { MidiStatus } from '../../core/domain/midi';
import type { MidiCommand, MidiPort } from '../../core/ports/MidiPort';

export function useMidiSession(port: MidiPort, opened: boolean) {
  const [status, setStatus] = useState<MidiStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const generation = useRef(0);
  const requests = useRef(0);
  const pending = useRef<number | null>(null);
  useEffect(() => {
    generation.current += 1;
    pending.current = null;
    setStatus(null);
    setBusy(false);
    setError(false);
    if (!opened) return;
    const owner = generation.current;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      const request = requests.current;
      const mutating = pending.current !== null;
      const current = () =>
        owner === generation.current && request === requests.current && !mutating;
      try {
        const next = await port.command('midi_status');
        if (current()) setStatus(next);
      } catch {
        if (current()) setError(true);
      } finally {
        if (owner === generation.current) timer = setTimeout(() => void poll(), 250);
      }
    };
    void poll();
    return () => {
      generation.current += 1;
      clearTimeout(timer);
    };
  }, [port, opened]);
  const command = async (name: MidiCommand, args?: Record<string, unknown>) => {
    const owner = generation.current;
    const request = ++requests.current;
    pending.current = request;
    setBusy(true);
    setError(false);
    try {
      const next = await port.command(name, args);
      if (owner === generation.current && request === requests.current) setStatus(next);
      return owner === generation.current && request === requests.current;
    } catch {
      if (owner === generation.current && request === requests.current) setError(true);
      return false;
    } finally {
      if (owner === generation.current && request === requests.current) {
        pending.current = null;
        setBusy(false);
      }
    }
  };
  return { status, busy, error, command };
}
