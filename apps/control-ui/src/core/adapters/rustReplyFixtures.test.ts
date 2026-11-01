import { readdirSync, readFileSync } from 'node:fs';
import { expect, it } from 'vitest';
import {
  controlStateSnapshot,
  graphCatalog,
  mixRelayStatus,
  runtimeGraphDocument,
} from './control/protocolDecoders';
import { TauriDriverPort } from './tauri/TauriDriverPort';
import { TauriLicensingPort } from './tauri/TauriLicensingPort';
import { TauriUpdatesPort } from './tauri/TauriUpdatesPort';

/** Replies serialized by their Rust owners; `pnpm fixtures:replies` regenerates them. */
const directory = new URL('../../../../../tests/fixtures/control-replies/', import.meta.url);
const fixture = (name: string): unknown =>
  JSON.parse(readFileSync(new URL(`${name}.json`, directory), 'utf8'));
const reply = (name: string) => async () => fixture(name);

const decoders: Record<string, (value: unknown) => Promise<unknown> | unknown> = {
  status: (value) => mixRelayStatus(value, 'status'),
  graph: (value) => runtimeGraphDocument(value, 'graph'),
  catalog: (value) => graphCatalog(value, 'catalog'),
  'telemetry-state': (value) => controlStateSnapshot(value, 'telemetry-state'),
  'driver-snapshot': () => new TauriDriverPort(reply('driver-snapshot')).getSnapshot(),
  'licensing-status': () => new TauriLicensingPort(reply('licensing-status')).getStatus(),
  updates: () => new TauriUpdatesPort(reply('updates')).check(),
};

it('decodes every reply fixture without changing it', async () => {
  for (const [name, decode] of Object.entries(decoders)) {
    const value = fixture(name);
    await expect(Promise.resolve(decode(value)), name).resolves.toEqual(value);
  }
});

it('has a decoder for every reply fixture the Rust owners write', () => {
  const names = readdirSync(directory).map((file) => file.replace(/\.json$/u, ''));
  expect(names.sort()).toEqual(Object.keys(decoders).sort());
});
