import { useEffect, useRef, useState } from 'react';
import type { ControlPort, RecorderStatus } from '../../core/control-port/types';
import { absoluteRecordingPath, recorderError } from './RecorderController';

/** Drafts belong to this node view; an async suggestion never replaces a manual choice. */
export function useRecordingPath(port: ControlPort, status: RecorderStatus | null) {
  const [draft, setDraft] = useState<string | null>(null);
  const defaults = useDesktopSuggestion(port, status, draft === null);
  const path = draft ?? (port.defaultRecordingPath ? defaults.path : status?.path) ?? '';
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const owner = useRef({ active: true, busy: false, port });
  useEffect(() => {
    const session = { active: true, busy: false, port };
    owner.current = session;
    setBusy(false);
    setError(null);
    return () => {
      session.active = false;
    };
  }, [port]);
  const choose = async (title: string) => {
    const session = owner.current;
    if (!session.port.selectRecordingPath || session.busy) return;
    session.busy = true;
    setBusy(true);
    setError(null);
    try {
      const selected = await session.port.selectRecordingPath({
        defaultPath: path || undefined,
        title,
      });
      if (session.active && selected !== null) setDraft(selected);
    } catch (cause) {
      if (session.active) setError(recorderError(cause));
    } finally {
      session.busy = false;
      if (session.active) setBusy(false);
    }
  };
  return {
    path,
    setPath: setDraft,
    busy,
    error: error ?? (draft === null ? defaults.error : null),
    choose,
  };
}

function useDesktopSuggestion(
  port: ControlPort,
  status: RecorderStatus | null,
  automatic: boolean,
) {
  const [path, setPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const finishedTake =
    status && (status.state === 'stopped' || status.state === 'failed')
      ? `${status.state}:${status.path ?? ''}:${status.frames}`
      : null;
  useEffect(() => {
    if (!automatic || finishedTake === null || !port.defaultRecordingPath) return;
    let active = true;
    setPath(null);
    setError(null);
    void port.defaultRecordingPath().then(
      (suggestion) => {
        if (!active) return;
        if (absoluteRecordingPath(suggestion)) setPath(suggestion);
        else setError('Desktop recording path must be absolute');
      },
      (cause: unknown) => {
        if (active) setError(recorderError(cause));
      },
    );
    return () => {
      active = false;
    };
  }, [port, automatic, finishedTake]);
  return { path, error };
}
