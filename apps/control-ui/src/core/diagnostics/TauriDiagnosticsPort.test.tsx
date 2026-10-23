import { invoke } from '@tauri-apps/api/core';
import { beforeEach, expect, it, vi } from 'vitest';

import { scanFailureCounts } from './DiagnosticsPort';
import { TauriDiagnosticsPort } from './TauriDiagnosticsPort';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

beforeEach(() => vi.mocked(invoke).mockReset());

it('sends only codes and scan counts to the native report command', async () => {
  vi.mocked(invoke).mockResolvedValue(undefined);
  const failure = { format: 'vst3' as const, stage: 'probe' as const, message: '/Users/name' };
  const context = {
    problems: ['graph_apply_timeout'],
    processorScan: scanFailureCounts([
      failure,
      failure,
      { ...failure, pluginName: 'Other' },
      { format: 'clap', stage: 'scan', message: 'denied' },
    ]),
  };
  await new TauriDiagnosticsPort().saveReport(context);
  expect(vi.mocked(invoke).mock.calls).toEqual([
    [
      'diagnostics_save_report',
      {
        context: {
          problems: ['graph_apply_timeout'],
          processorScan: [
            { format: 'vst3', stage: 'probe', count: 2 },
            { format: 'clap', stage: 'scan', count: 1 },
          ],
        },
      },
    ],
  ]);
  expect(JSON.stringify(context)).not.toMatch(/Users|Other|denied/u);
});

it('does not create a report port in a browser', async () => {
  const original = Object.getOwnPropertyDescriptor(window, '__TAURI_INTERNALS__');
  try {
    Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
    vi.resetModules();
    expect((await import('./TauriDiagnosticsPort')).localDiagnosticsPort).toBeNull();
    Object.defineProperty(window, '__TAURI_INTERNALS__', { configurable: true, value: {} });
    vi.resetModules();
    const native = await import('./TauriDiagnosticsPort');
    expect(native.localDiagnosticsPort).toBeInstanceOf(native.TauriDiagnosticsPort);
  } finally {
    if (original) Object.defineProperty(window, '__TAURI_INTERNALS__', original);
    else Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
  }
});
