export type UpdateProduct = 'mix-relay';

export interface UpdateDownload {
  os: string;
  arch: string;
  url: string;
  sha256: string;
  size: number;
}

export interface ProductUpdate {
  product: UpdateProduct;
  installedVersion: string | null;
  latestVersion: string | null;
  state: 'available' | 'current' | 'published' | 'not_published' | 'unavailable';
  downloads: UpdateDownload[];
  notesUrl: string | null;
}

/** Desktop checks and installer opening stay behind the native catalog boundary. */
export interface UpdatesPort {
  check(): Promise<ProductUpdate[]>;
  openDownload(product: UpdateProduct, url: string): Promise<void>;
}
