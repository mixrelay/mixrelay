import { render, screen } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { expect, it, vi } from 'vitest';
import { StorybookMockControlPort } from '../../core/control-port/StorybookMockControlPort';
import type { AudioSessionStatus, ToneNariStatus } from '../../core/control-port/types';
import { i18n } from '../../core/i18n';
import { en } from '../../core/i18n/locales/en';
import { koKR } from '../../core/i18n/locales/ko-KR';
import { pseudo } from '../../core/i18n/locales/pseudo';
import { ToneNariProvider } from '../../core/theme/ToneNariProvider';
import { AudioSessionControl } from './AudioSessionControl';

const hiddenDiagnostics = [
  'backend',
  'deviceRate',
  'processingRate',
  'clock',
  'conversion',
  'drift',
  'syncNotice',
  'changeNotice',
];

it('retains recorder finalization errors even without an audio session or recorder node', async () => {
  await i18n.changeLanguage('en');
  const status: ToneNariStatus = {
    protocolVersion: 1,
    driverState: 'ready',
    sampleRate: null,
    bufferFrames: null,
    underruns: 0,
    overruns: 0,
    recorderError: 'take.wav: Disk full',
  };
  const controlPort = new StorybookMockControlPort({
    status,
    graph: { schemaVersion: 2, revision: 1, nodes: [], edges: [] },
    graphStatus: { protocolVersion: 1, activeRevision: 1, submittedRevision: 1 },
  });
  render(
    <ToneNariProvider>
      <I18nextProvider i18n={i18n}>
        <AudioSessionControl
          status={status}
          controlPort={controlPort}
          onApplied={vi.fn()}
          onBusyChange={vi.fn()}
        />
      </I18nextProvider>
    </ToneNariProvider>,
  );
  expect(screen.getByRole('alert')).toHaveTextContent('Recording error: take.wav: Disk full');
});

it.each(
  ['en', 'ko-KR', 'pseudo'].flatMap((language) =>
    [null, { sampleRate: 48000, blockFrames: 256 }, { sampleRate: 44100, blockFrames: 128 }].map(
      (preferredFormat) => ({ language, preferredFormat }),
    ),
  ),
)(
  'does not expose separate active/saved settings: $language $preferredFormat',
  async ({ language, preferredFormat }) => {
    await i18n.changeLanguage(language);
    const status: ToneNariStatus = {
      protocolVersion: 1,
      driverState: 'ready',
      sampleRate: 48000,
      bufferFrames: 256,
      underruns: 0,
      overruns: 0,
      audioSession: {
        format: { sampleRate: 48000, blockFrames: 256 },
        preferredFormat,
        clockAuthority: 'engine_timer',
        observedDeviceSampleRate: 48000,
        issue: 'stored_format_unavailable',
        transportLatencyFrames: null,
        capabilities: {
          backend: 'core_audio',
          supportedSampleRates: [44100, 48000],
          supportedBlockFrames: [128, 256],
          canReconfigureLive: true,
          hardwareInput: true,
          hardwareOutput: true,
          virtualEndpoint: true,
          applicationCapture: true,
          sampleRateConversion: false,
          driftCorrection: false,
        },
      },
    };
    const controlPort = new StorybookMockControlPort({
      status,
      graph: { schemaVersion: 2, revision: 1, nodes: [], edges: [] },
      graphStatus: { protocolVersion: 1, activeRevision: 1, submittedRevision: 1 },
    });
    render(
      <I18nextProvider i18n={i18n}>
        <ToneNariProvider>
          <AudioSessionControl
            status={status}
            controlPort={controlPort}
            onApplied={vi.fn()}
            onBusyChange={vi.fn()}
          />
        </ToneNariProvider>
      </I18nextProvider>,
    );
    expect(screen.queryByText(i18n.t('audioSession.diagnostics'))).not.toBeInTheDocument();
    expect(screen.getByText(`${i18n.t('audioSession.bufferLatency')} 5.33 ms`)).toBeInTheDocument();
    // Keep one editable settings form; native and persisted values remain an internal contract.
    const visibleInput = { selector: 'select' };
    expect(screen.queryByLabelText(i18n.t('audioSession.sampleRate'), visibleInput)).toBeNull();
    expect(
      screen.getAllByLabelText(i18n.t('audioSession.processingBuffer'), visibleInput),
    ).toHaveLength(1);
    expect(
      screen.getByLabelText(i18n.t('audioSession.processingBuffer'), visibleInput),
    ).toHaveValue('256');
    expect(screen.getAllByText(i18n.t('audioSession.processingBuffer'))).toHaveLength(1);
    for (const key of hiddenDiagnostics)
      expect(screen.queryByText(i18n.t(`audioSession.${key}`))).not.toBeInTheDocument();
    for (const label of [
      'Active format',
      'Saved format',
      'Not saved',
      '현재 적용값',
      '저장된 설정',
      '저장되지 않음',
      '[Active format]',
      '[Saved format]',
      '[Not saved]',
      'Block size',
      'Device rate',
      '블록 크기',
      '장치 레이트',
    ])
      expect(screen.queryByText(label)).not.toBeInTheDocument();
    // Simplification must not hide a failed application or change the stored/runtime values.
    expect(
      screen.getByText(i18n.t('audioSession.issues.stored_format_unavailable')),
    ).toBeInTheDocument();
    expect(status.audioSession?.preferredFormat).toEqual(preferredFormat);
    expect(status.audioSession?.format).toEqual({ sampleRate: 48000, blockFrames: 256 });
  },
);

it.each(
  ['en', 'ko-KR', 'pseudo'].flatMap((language) =>
    ([null, 'clock_not_synchronized', 'block_size_mismatch'] as const).map((issue) => ({
      language,
      issue,
    })),
  ),
)(
  'shows persistence failure independently of session issue $issue in $language',
  async ({ language, issue }) => {
    await i18n.changeLanguage(language);
    const session: AudioSessionStatus = {
      endpointPreferencesUnavailable: true,
      format: { sampleRate: 48000, blockFrames: 128 },
      clockAuthority: 'engine_timer',
      observedDeviceSampleRate: null,
      observedDeviceBufferFrames: issue === 'block_size_mismatch' ? 512 : null,
      issue,
      transportLatencyFrames: null,
      capabilities: {
        backend: 'contract_only',
        supportedSampleRates: [48000],
        supportedBlockFrames: [128],
        canReconfigureLive: false,
        hardwareInput: false,
        hardwareOutput: false,
        virtualEndpoint: true,
        applicationCapture: false,
        sampleRateConversion: false,
        driftCorrection: false,
      },
    };
    const status: ToneNariStatus = {
      protocolVersion: 1,
      driverState: 'ready',
      sampleRate: 48000,
      bufferFrames: 128,
      underruns: 0,
      overruns: 0,
      audioSession: session,
      recorderError: 'Could not finalize take.wav',
    };
    const controlPort = new StorybookMockControlPort({
      status,
      graph: { schemaVersion: 2, revision: 1, nodes: [], edges: [] },
      graphStatus: { protocolVersion: 1, activeRevision: 1, submittedRevision: 1 },
    });
    render(
      <I18nextProvider i18n={i18n}>
        <ToneNariProvider>
          <AudioSessionControl
            status={status}
            controlPort={controlPort}
            onApplied={vi.fn()}
            onBusyChange={vi.fn()}
          />
        </ToneNariProvider>
      </I18nextProvider>,
    );
    expect(
      await screen.findByText(i18n.t('audioSession.issues.endpoint_preferences_unavailable')),
    ).toBeInTheDocument();
    if (issue) expect(screen.getByText(i18n.t(`audioSession.issues.${issue}`))).toBeInTheDocument();
    expect(
      screen.getByText(`${i18n.t('recorder.failed')}: Could not finalize take.wav`),
    ).toBeInTheDocument();
    for (const locale of [en, koKR, pseudo])
      expect(locale.audioSession.issues.endpoint_preferences_unavailable).toBeTruthy();
  },
);
