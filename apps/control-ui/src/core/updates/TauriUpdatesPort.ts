import { invoke } from '@tauri-apps/api/core';
import { array, integer, literal, nullable, object, string } from '../adapters/decode';

import type { ProductUpdate, UpdateDownload, UpdateProduct, UpdatesPort } from './UpdatesPort';

const productUpdates = array(
  object<ProductUpdate>({
    product: literal('mix-relay'),
    installedVersion: nullable(string),
    latestVersion: nullable(string),
    state: literal('available', 'current', 'published', 'not_published', 'unavailable'),
    downloads: array(
      object<UpdateDownload>({
        os: string,
        arch: string,
        url: string,
        sha256: string,
        size: integer,
      }),
    ),
    notesUrl: nullable(string),
  }),
);

export class TauriUpdatesPort implements UpdatesPort {
  async check(): Promise<ProductUpdate[]> {
    return productUpdates(await invoke('updates_check'), 'updates_check');
  }

  async openDownload(product: UpdateProduct, url: string): Promise<void> {
    await invoke('updates_open_download', { product, url });
  }
}

/** Browser and LAN clients never query the external update service. */
export const localUpdatesPort: UpdatesPort | null =
  '__TAURI_INTERNALS__' in window ? new TauriUpdatesPort() : null;
