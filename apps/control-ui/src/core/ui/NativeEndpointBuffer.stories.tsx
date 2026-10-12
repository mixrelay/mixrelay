import type { Meta, StoryObj } from '@storybook/react-vite';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';

import type { NativeSampleFormat } from '../control-port/types';
import { ActionButton } from './ActionButton';
import { AudioNodeCard, type EndpointControl } from './AudioNodeCard';
import { NativeEndpointBuffer } from './NativeEndpointBuffer';
import { Stack } from './Stack';
import { Text } from './Text';

function DeviceNode({ pending = false, unused = false }: { pending?: boolean; unused?: boolean }) {
  const { t } = useTranslation();
  const [frames, setFrames] = useState(128);
  const [sampleRate, setSampleRate] = useState(48000);
  const [sampleFormat, setSampleFormat] = useState<NativeSampleFormat>('float32');
  const endpointControl: EndpointControl = {
    displayName: 'MixRelay IO',
    nameLabel: t('driverManagement.name'),
    channelsLabel: t('driverManagement.channels'),
    canRename: false,
    renderChannels: 2,
    captureChannels: 0,
    allowedRenderChannels: [2],
    allowedCaptureChannels: [2],
    observedRate: { label: t('audioSession.sampleRate'), value: sampleRate },
    sampleRateOptions: pending ? undefined : [44100, 48000, 96000],
    nativeFormats: {
      render: {
        current: { sampleRate, channels: 2, sampleFormat },
        supported: ['signed16', 'signed24', 'signed32', 'float32'].map((sampleFormat) => ({
          sampleRate,
          channels: 2,
          sampleFormat: sampleFormat as NativeSampleFormat,
        })),
      },
      capture: { current: null, supported: [] },
    },
    nativeFormatLabels: {
      bitDepth: t('driverManagement.bitDepth'),
      unknown: t('driverManagement.unknown'),
      mixed: t('driverManagement.mixed'),
    },
    formatsDisabled: pending,
    bufferControl: {
      label: t('driverManagement.bufferSize'),
      framesLabel: t('driverManagement.frames'),
      currentFrames: unused ? null : frames,
      preferredFrames: unused ? frames : null,
      unselectedLabel: t('driverManagement.unselectedBuffer'),
      pendingLabel: t('driverManagement.pendingBuffer'),
      observedLabel: t('driverManagement.observedBuffer'),
      options: [32, 64, 128, 256, 512, 1024, 2048],
      disabled: pending,
    },
  };
  return (
    <div style={{ width: 'var(--node-default-width)' }}>
      <AudioNodeCard
        category="Device"
        icon="IO"
        title="MixRelay IO"
        inputCount={2}
        outputCount={0}
        endpointControl={endpointControl}
        showPorts={false}
        onEndpointChange={(update) => {
          if (update.bufferFrames !== undefined) setFrames(update.bufferFrames);
          if (update.sampleRate !== undefined) setSampleRate(update.sampleRate);
          if (update.sampleFormat !== undefined) setSampleFormat(update.sampleFormat);
        }}
      />
    </div>
  );
}

const meta = {
  title: 'Core/Audio Nodes/Buffer Size',
  component: DeviceNode,
  render: () => <DeviceNode />,
} satisfies Meta<typeof DeviceNode>;
export default meta;
type Story = StoryObj<typeof meta>;

export const DeviceOutput: Story = {};
export const Pending: Story = { render: () => <DeviceNode pending /> };
export const SavedBeforeConnection: Story = { render: () => <DeviceNode unused /> };
export const NarrowLongTranslation: Story = {
  globals: { locale: 'pseudo' },
  parameters: { viewport: { value: 'mobile' } },
  render: () => (
    <div style={{ width: 260 }}>
      <DeviceNode unused />
    </div>
  ),
};

/** Request and acknowledgement are separate so readback behavior stays visible. */
function BufferReadbackExample({
  mode,
}: {
  mode: 'mismatch' | 'unsupported' | 'unknown' | 'error';
}) {
  const { t } = useTranslation();
  const [preferred, setPreferred] = useState<number | null>(mode === 'mismatch' ? 256 : null);
  const [current, setCurrent] = useState<number | null>(
    mode === 'unknown' ? null : mode === 'unsupported' ? 192 : 128,
  );
  return (
    <div style={{ width: 320, maxWidth: '100%' }}>
      <Stack>
        <Text kind="supporting">
          선택은 요청값, 아래 상태 문구는 실제 관측값입니다. 확인 버튼은 샘플 Driver 응답만
          반영합니다.
        </Text>
        <NativeEndpointBuffer
          control={{
            label: t('driverManagement.bufferSize'),
            framesLabel: t('driverManagement.frames'),
            currentFrames: current,
            preferredFrames: preferred,
            options: mode === 'error' ? [] : [64, 128, 256, 512],
            disabled: mode === 'error',
            error: mode === 'error' ? t('driverManagement.changeFailed') : null,
            unselectedLabel: t('driverManagement.unselectedBuffer'),
            pendingLabel: t('driverManagement.pendingBuffer'),
            observedLabel: t('driverManagement.observedBuffer'),
          }}
          onChange={setPreferred}
        />
        <ActionButton
          disabled={preferred === null || mode === 'error'}
          onClick={() => {
            setCurrent(preferred);
            setPreferred(null);
          }}
        >
          샘플 Driver 응답 확인
        </ActionButton>
      </Stack>
    </div>
  );
}
export const ReadbackMismatch: Story = { render: () => <BufferReadbackExample mode="mismatch" /> };
export const UnsupportedCurrent: Story = {
  render: () => <BufferReadbackExample mode="unsupported" />,
};
export const NoReadback: Story = { render: () => <BufferReadbackExample mode="unknown" /> };
export const Rejected: Story = { render: () => <BufferReadbackExample mode="error" /> };
