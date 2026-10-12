import type { AudioGraphModel } from '../../audio-graph/model';
import { AudioMeterStore } from '../../audio-meter/AudioMeterStore';
import type { NativeDeviceFormats } from '../../control-port/types';
import type { DynamicsMeterContract } from '../../ui/DynamicsMeter';
import type { GraphicEqualizerContract } from '../../ui/GraphicEqualizer';
import type { EndpointBufferControl } from '../../ui/NativeEndpointBuffer';
import type { ParameterFieldControl } from '../../ui/ParameterField';

export const sampleGraph: AudioGraphModel = {
  revision: 1,
  nodes: [
    {
      id: 'source',
      position: { x: 24, y: 48 },
      inputPorts: [],
      outputPorts: [{ id: 'out-1', label: 'Output CH 1', channelIndex: 0 }],
      appearance: { category: 'Source', icon: 'MIC', title: 'Sample microphone', channelCount: 1 },
    },
    {
      id: 'sink',
      position: { x: 320, y: 48 },
      outputPorts: [],
      inputPorts: [{ id: 'in-1', label: 'Input CH 1', channelIndex: 0 }],
      appearance: { category: 'Output', icon: 'IO', title: 'Sample monitor', channelCount: 1 },
    },
  ],
  edges: [
    {
      id: 'sample-edge',
      sourceNodeId: 'source',
      sourcePortId: 'out-1',
      targetNodeId: 'sink',
      targetPortId: 'in-1',
    },
  ],
};
export const gainControl: ParameterFieldControl = {
  id: 'gain',
  label: '게인',
  minimum: -60,
  maximum: 12,
  step: 0.5,
  suffix: 'dB',
  value: 0,
};
export const bufferControl: EndpointBufferControl = {
  label: '출력 버퍼',
  framesLabel: 'frames',
  currentFrames: 128,
  preferredFrames: 128,
  unselectedLabel: '미확인',
  pendingLabel: '적용 확인 중',
  observedLabel: '현재 확인값',
  options: [64, 128, 256, 512],
  disabled: false,
};
const float = { sampleRate: 48000, channels: 2, sampleFormat: 'float32' as const };
const pcm = { ...float, sampleFormat: 'signed24' as const };
export const sampleFormats: NativeDeviceFormats = {
  render: { current: float, supported: [float, pcm] },
  capture: { current: float, supported: [float, pcm] },
};
export const dynamicsContract: DynamicsMeterContract = {
  nodeId: 'sample-compressor',
  portId: 'audio',
  thresholdId: 'thresholdDb',
  inputLabel: '입력',
  outputLabel: '출력',
  reductionLabel: '게인 감쇠',
  unavailableLabel: '계측 미수신',
};
export const thresholdControls: ParameterFieldControl[] = [
  {
    id: 'thresholdDb',
    label: '임계값',
    value: -18,
    minimum: -60,
    maximum: 0,
    step: 0.5,
    suffix: 'dB',
  },
];
export const sampleMeterStore = new AudioMeterStore();
sampleMeterStore.update({
  protocolVersion: 1,
  revision: 1,
  nodes: [
    {
      nodeId: 'sample-compressor',
      portId: 'audio',
      peaks: [0.5, 0.25],
      dynamics: { inputPeaks: [1, 0.5], gainReductionDb: 12, thresholdDb: -18 },
    },
  ],
});
export const equalizerContract: GraphicEqualizerContract = {
  sampleRate: 48000,
  bands: ['low', 'mid', 'high'].map((id, index) => ({
    id,
    label: ['Low', 'Mid', 'High'][index] ?? id,
    frequencyId: `${id}-frequency`,
    gainId: `${id}-gain`,
    qId: `${id}-q`,
  })),
};
export const equalizerControls: ParameterFieldControl[] = ['low', 'mid', 'high'].flatMap(
  (id, index) => [
    {
      id: `${id}-frequency`,
      label: '주파수',
      value: [150, 1000, 6000][index] ?? 1000,
      minimum: 20,
      maximum: 20000,
      step: 1,
      suffix: 'Hz',
      scale: 'logarithmic',
    },
    {
      id: `${id}-gain`,
      label: '게인',
      value: 0,
      minimum: -18,
      maximum: 18,
      step: 0.5,
      suffix: 'dB',
    },
    {
      id: `${id}-q`,
      label: 'Q',
      value: Math.SQRT1_2,
      minimum: 0.1,
      maximum: 10,
      step: 0.001,
      suffix: '',
    },
  ],
);
