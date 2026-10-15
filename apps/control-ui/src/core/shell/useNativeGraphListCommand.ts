import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useEffect, useRef } from 'react';

/**
 * Binds View → Show/Hide Graph List (⌃⌘S): the native command toggles the list, and the
 * list's visibility keeps the command label naming what it will do next.
 */
export function useNativeGraphListCommand(visible: boolean, onToggle: () => void) {
  const toggle = useRef(onToggle);
  toggle.current = onToggle;
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let stop: (() => void) | undefined;
    void listen('graph-list-toggle', () => toggle.current())
      .then((unsubscribe) => {
        if (disposed) unsubscribe();
        else stop = unsubscribe;
      })
      .catch((error: unknown) => console.error('Graph list menu listener failed', error));
    return () => {
      disposed = true;
      stop?.();
    };
  }, []);
  useEffect(() => {
    if (!isTauri()) return;
    void invoke('set_graph_list_visible', { visible }).catch((error: unknown) =>
      console.error('Graph list menu label failed', error),
    );
  }, [visible]);
}
