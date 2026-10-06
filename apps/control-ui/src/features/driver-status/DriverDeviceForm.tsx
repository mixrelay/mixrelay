import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { VirtualEndpointDevice, VirtualEndpointSettings, VirtualEndpointSpec } from '../../core/control-port/types';
import { nativeFormatKey, nativeFormatLabel } from '../../core/driver/nativeFormatLabel';
import { ActionButton } from '../../core/ui/ActionButton';
import { SelectField } from '../../core/ui/SelectField';
import { TextField } from '../../core/ui/TextField';
import styles from './DriverManagement.module.css';
import type { DriverSession } from './useDriverSession';

function width(spec: VirtualEndpointSpec, side: 'render' | 'capture') {
  return spec[side]?.channels ?? 0;
}

export function DriverDeviceForm({ spec, settings, busy, creating, onSave }: {
  spec: VirtualEndpointSpec;
  settings: VirtualEndpointSettings;
  busy: boolean;
  creating: boolean;
  onSave: (spec: VirtualEndpointSpec) => Promise<unknown>;
}) {
  const { t } = useTranslation();
  const [name, setName] = useState(spec.displayName);
  const [render, setRender] = useState(width(spec, 'render'));
  const [capture, setCapture] = useState(width(spec, 'capture'));
  const [rate, setRate] = useState(spec.defaultSampleRate);
  useEffect(() => {
    setName(spec.displayName);
    setRender(width(spec, 'render'));
    setCapture(width(spec, 'capture'));
    setRate(spec.defaultSampleRate);
  }, [spec.displayName, spec.render?.channels, spec.capture?.channels, spec.defaultSampleRate]);
  const changed = name.trim() !== spec.displayName || render !== width(spec, 'render') ||
    capture !== width(spec, 'capture') || rate !== spec.defaultSampleRate;
  const choices = (values: number[], current: number) => [...new Set([...values, current])]
    .map((value) => ({ value: String(value), label: String(value) }));
  return <form className={styles.fields} onSubmit={(event) => {
    event.preventDefault();
    if (!name.trim() || busy || (!creating && !changed)) return;
    void onSave({ ...spec, displayName: name.trim(), defaultSampleRate: rate,
      render: spec.render && render !== spec.render.channels
        ? { channels: render, channelLayout: render === 1 ? 'mono' : render === 2 ? 'stereo' : 'discrete' } : spec.render,
      capture: spec.capture && capture !== spec.capture.channels
        ? { channels: capture, channelLayout: capture === 1 ? 'mono' : capture === 2 ? 'stereo' : 'discrete' } : spec.capture });
  }}>
    <TextField label={t('driverManagement.name')} value={name} maxLength={40}
      readOnly={!creating && !settings.canRename} disabled={busy}
      onChange={(event) => setName(event.currentTarget.value)} />
    <div className={styles.channels}>
      {spec.render ? <SelectField label={t('driverManagement.renderChannels')} value={String(render)}
        data={choices(settings.renderChannels, render)} disabled={busy || settings.renderChannels.length < 2}
        onChange={(value) => setRender(Number(value))} /> : null}
      {spec.capture ? <SelectField label={t('driverManagement.captureChannels')} value={String(capture)}
        data={choices(settings.captureChannels, capture)} disabled={busy || settings.captureChannels.length < 2}
        onChange={(value) => setCapture(Number(value))} /> : null}
    </div>
    {spec.supportedSampleRates.length > 0 ? <SelectField label={t('audioSession.sampleRate')} value={String(rate)}
      data={spec.supportedSampleRates.map((value) => ({ value: String(value), label: `${value / 1_000} kHz` }))}
      disabled={busy || spec.supportedSampleRates.length < 2} onChange={(value) => setRate(Number(value))} /> : null}
    <div className={styles.actions}>
      <ActionButton type="submit" size="xs" disabled={busy || !name.trim() || (!creating && !changed)}>
        {t(creating ? 'common.create' : 'driverManagement.save')}
      </ActionButton>
    </div>
  </form>;
}

export function DriverNativeFormats({ device, session }: { device: VirtualEndpointDevice; session: DriverSession }) {
  const { t } = useTranslation();
  return <div className={styles.fields}>
    {(['render', 'capture'] as const).map((direction) => {
      if (!device.spec[direction]) return null;
      const formats = device.nativeFormats[direction];
      const current = formats.current;
      if (!current) return <TextField key={direction} readOnly label={t(`driverManagement.${direction}Format`)} value={t('driverManagement.unknown')} />;
      const data = [...formats.supported];
      if (!data.some((format) => nativeFormatKey(format) === nativeFormatKey(current))) data.push(current);
      return <SelectField key={direction} label={t(`driverManagement.${direction}Format`)}
        value={nativeFormatKey(current)} data={data.map((format) => ({ value: nativeFormatKey(format), label: nativeFormatLabel(format) }))}
        disabled={session.busy || session.snapshot?.state !== 'ready' || formats.supported.length === 0}
        onChange={(value) => {
          const format = formats.supported.find((candidate) => nativeFormatKey(candidate) === value);
          if (format && value !== nativeFormatKey(current))
            void session.mutate({ action: 'set_format', endpointId: device.spec.id, direction, format });
        }} />;
    })}
  </div>;
}
