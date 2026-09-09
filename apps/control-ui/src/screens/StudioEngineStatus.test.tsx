import { fireEvent, render, screen } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { describe, expect, it, vi } from 'vitest';
import { StorybookMockControlPort } from '../core/control-port/StorybookMockControlPort';
import type { AudioSessionStatus, ToneNariStatus } from '../core/control-port/types';
import { i18n } from '../core/i18n';
import { ToneNariProvider } from '../core/theme/ToneNariProvider';
import { StudioScreen } from './StudioScreen';

vi.mock('../features/routing-workspace/RoutingWorkspace', () => ({ RoutingWorkspace: () => null }));
vi.mock('../features/audio-session/AudioSessionControl', () => ({
  AudioSessionControl: () => null,
}));

const session: AudioSessionStatus = {
  processingState: 'running',
  issue: null,
  format: { sampleRate: 48000, blockFrames: 128 },
  clockAuthority: 'engine_timer',
  observedDeviceSampleRate: null,
  transportLatencyFrames: null,
  capabilities: {
    backend: 'core_audio',
    supportedSampleRates: [48000],
    supportedBlockFrames: [128],
    canReconfigureLive: false,
    hardwareInput: true,
    hardwareOutput: true,
    virtualEndpoint: true,
    applicationCapture: true,
    sampleRateConversion: false,
    driftCorrection: false,
  },
};
const healthy: ToneNariStatus = {
  protocolVersion: 1,
  driverState: 'ready',
  sampleRate: 48000,
  bufferFrames: 128,
  underruns: 0,
  overruns: 0,
  audioSession: session,
};
type Case = [string, ToneNariStatus | null, string, string];
const cases: Case[] = [
  ['healthy', healthy, 'positive', 'routing.nodeStatus.running'],
  [
    'stopped',
    { ...healthy, audioSession: { ...session, processingState: 'stopped' } },
    'critical',
    'routing.nodeStatus.stopped',
  ],
  [
    'processing failed',
    { ...healthy, audioSession: { ...session, issue: 'processing_failed' } },
    'critical',
    'routing.nodeStatus.failed',
  ],
  [
    'unsynchronized clock',
    { ...healthy, audioSession: { ...session, issue: 'clock_not_synchronized' } },
    'warning',
    'routing.nodeStatus.issues.clock_not_synchronized',
  ],
  [
    'native buffer mismatch',
    {
      ...healthy,
      audioSession: { ...session, observedDeviceBufferFrames: 512, issue: 'block_size_mismatch' },
    },
    'warning',
    'routing.nodeStatus.issues.block_size_mismatch',
  ],
  [
    'missing app',
    { ...healthy, audioSession: { ...session, issue: 'application_unavailable' } },
    'warning',
    'routing.nodeStatus.issues.application_unavailable',
  ],
  [
    'ambiguous app',
    { ...healthy, audioSession: { ...session, issue: 'application_ambiguous' } },
    'warning',
    'routing.nodeStatus.issues.application_ambiguous',
  ],
  [
    'device unavailable',
    { ...healthy, audioSession: { ...session, issue: 'device_unavailable' } },
    'warning',
    'routing.nodeStatus.issues.device_unavailable',
  ],
  [
    'settings warning',
    { ...healthy, audioSession: { ...session, endpointPreferencesUnavailable: true } },
    'warning',
    'routing.nodeStatus.issues.preferences_unavailable',
  ],
  [
    'idle',
    { ...healthy, audioSession: { ...session, processingState: 'idle' } },
    'warning',
    'routing.nodeStatus.idle',
  ],
  [
    'old engine',
    { ...healthy, audioSession: { ...session, processingState: undefined } },
    'warning',
    'routing.nodeStatus.unknown',
  ],
  [
    'missing session',
    { ...healthy, audioSession: undefined },
    'warning',
    'routing.nodeStatus.unknown',
  ],
  ['unavailable transport', null, 'warning', 'routing.nodeStatus.unknown'],
  [
    'unavailable driver',
    { ...healthy, driverState: 'unavailable' },
    'critical',
    'driver.states.unavailable',
  ],
  ['blocked driver', { ...healthy, driverState: 'blocked' }, 'critical', 'driver.states.blocked'],
  [
    'installing driver',
    { ...healthy, driverState: 'installing' },
    'warning',
    'driver.states.installing',
  ],
];

function fixture(status: ToneNariStatus | null) {
  const port = new StorybookMockControlPort({
    graph: { schemaVersion: 2, revision: 1, nodes: [], edges: [] },
    graphStatus: { protocolVersion: 1, activeRevision: 1, submittedRevision: 1 },
    status: status ?? healthy,
  });
  if (!status) vi.spyOn(port, 'getStatus').mockRejectedValue(new Error('offline'));
  return render(
    <ToneNariProvider>
      <I18nextProvider i18n={i18n}>
        <StudioScreen controlPort={port} />
      </I18nextProvider>
    </ToneNariProvider>,
  );
}

describe.each(['en', 'ko-KR', 'pseudo'])('Studio footer health in %s', (locale) => {
  it.each(cases)(
    'uses one consistent accessible label and color for %s',
    async (_name, status, tone, key) => {
      await i18n.changeLanguage(locale);
      const view = fixture(status);
      const label = i18n.t(key);
      expect(label).not.toBe(key);
      const marker = await screen.findByRole('img', { name: label });
      expect(marker).toHaveAttribute('data-tone', tone);
      expect(screen.queryByText(i18n.t('driver.states.ready'))).not.toBeInTheDocument();
      expect(screen.queryByText(label)).not.toBeInTheDocument();
      if (_name === 'unsynchronized clock') {
        fireEvent.mouseEnter(marker);
        expect(await screen.findByRole('tooltip')).toHaveTextContent(label);
      }
      view.unmount();
    },
  );
});
