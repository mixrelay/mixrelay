import { invoke } from '@tauri-apps/api/core';

import type { ProductUpdate, UpdateProduct, UpdatesPort } from './UpdatesPort';

export class TauriUpdatesPort implements UpdatesPort {
  check(): Promise<ProductUpdate[]> {
    return invoke<ProductUpdate[]>('updates_check');
  }

  openDownload(product: UpdateProduct, url: string): Promise<void> {
    return invoke<void>('updates_open_download', { product, url });
  }
}

/** Browser and LAN clients never query the external update service. */
export const localUpdatesPort: UpdatesPort | null =
  '__TAURI_INTERNALS__' in window ? new TauriUpdatesPort() : null;
