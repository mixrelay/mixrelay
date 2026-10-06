import { useCallback, useEffect, useRef, useState } from 'react';

import type { ProductUpdate, UpdateProduct, UpdatesPort } from '../../core/updates/UpdatesPort';

const checkInterval = 24 * 60 * 60 * 1_000;

interface UpdateSession {
  port: UpdatesPort;
  active: boolean;
  checking: boolean;
  opening: boolean;
}

interface UpdateState {
  port: UpdatesPort | null;
  products: ProductUpdate[];
  checked: boolean;
  checking: boolean;
  failed: boolean;
  opening: string | null;
  openFailed: boolean;
}

function initialState(port: UpdatesPort | null): UpdateState {
  return {
    port,
    products: [],
    checked: false,
    checking: false,
    failed: false,
    opening: null,
    openFailed: false,
  };
}

export function useUpdatesSession(port: UpdatesPort | null) {
  const [state, setState] = useState(() => initialState(port));
  const session = useRef<UpdateSession | null>(null);
  const owner = useRef(port);
  owner.current = port;

  const refresh = useCallback(async () => {
    const current = session.current;
    if (
      !current || !current.active || current.port !== port || current.checking || current.opening
    ) return;
    current.checking = true;
    setState((value) => ({ ...value, checking: true, openFailed: false }));
    try {
      const products = await current.port.check();
      if (current.active && owner.current === current.port) {
        setState((value) => ({ ...value, products, checked: true, failed: false }));
      }
    } catch {
      if (current.active && owner.current === current.port) {
        setState((value) => ({
          ...value,
          products: value.products.map((product) => ({
            ...product,
            state: 'unavailable',
            latestVersion: null,
            downloads: [],
            notesUrl: null,
          })),
          checked: true,
          failed: true,
        }));
      }
    } finally {
      current.checking = false;
      if (current.active && owner.current === current.port) {
        setState((value) => ({ ...value, checking: false }));
      }
    }
  }, [port]);

  useEffect(() => {
    setState(initialState(port));
    if (!port) {
      session.current = null;
      return;
    }
    const current: UpdateSession = { port, active: true, checking: false, opening: false };
    session.current = current;
    // Let React's development effect replay finish before sending the native request.
    void Promise.resolve().then(() => {
      if (current.active) void refresh();
    });
    const timer = setInterval(() => void refresh(), checkInterval);
    return () => {
      current.active = false;
      clearInterval(timer);
    };
  }, [port, refresh]);

  const openDownload = useCallback(
    async (product: UpdateProduct, url: string) => {
      const current = session.current;
      if (
        !current || !current.active || current.port !== port || current.opening || current.checking
      ) return;
      current.opening = true;
      setState((value) => ({ ...value, opening: url, openFailed: false }));
      try {
        await current.port.openDownload(product, url);
      } catch {
        if (current.active && owner.current === current.port) {
          setState((value) => ({ ...value, openFailed: true }));
        }
      } finally {
        current.opening = false;
        if (current.active && owner.current === current.port) {
          setState((value) => ({ ...value, opening: null }));
        }
      }
    },
    [port],
  );

  return { ...(state.port === port ? state : initialState(port)), refresh, openDownload };
}
