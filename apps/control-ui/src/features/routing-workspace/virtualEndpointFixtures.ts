import type {
  NativeDeviceFormats,
  VirtualEndpointSettings,
  VirtualEndpointSpec,
} from '../../core/control-port/types';

export const unknownNativeFormats: NativeDeviceFormats = {
  render: { current: null, supported: [] },
  capture: { current: null, supported: [] },
};

export const virtualSpec: VirtualEndpointSpec = {
  id: '095af572-6846-4ce6-b498-6dbeddb07435',
  displayName: 'SPAT send-return',
  render: { channels: 64, channelLayout: 'discrete' },
  capture: { channels: 16, channelLayout: 'discrete' },
  defaultSampleRate: 44100,
  supportedSampleRates: [44100, 48000],
};

export const virtualPolicy: VirtualEndpointSettings = {
  canCreate: true,
  canRename: true,
  renderChannels: [1, 2, 8, 16, 32, 64],
  captureChannels: [1, 2, 8, 16, 32, 64],
  creationTemplate: {
    render: virtualSpec.render,
    capture: virtualSpec.capture,
    defaultSampleRate: virtualSpec.defaultSampleRate,
    supportedSampleRates: virtualSpec.supportedSampleRates,
  },
  devices: [
    {
      nativeDeviceIds: [],
      nativeFormats: unknownNativeFormats,
      spec: virtualSpec,
      observedSampleRate: 48000,
      supportedSampleRates: [44100, 48000],
      canSetSampleRate: true,
    },
  ],
};

export const duplexReference = { render: true, capture: true };
