import { useEffect, useMemo, useSyncExternalStore } from 'react';
import { useTranslation } from 'react-i18next';
import type {
  ControlPort,
  RecorderEncoding,
  RecorderRequest,
  RecorderStatus,
} from '../../core/control-port/types';
import { NodeOptionsSection } from '../../core/ui/NodeOptionsSection';
import { useNodeOptionsState } from '../../core/ui/nodeOptionsState';
import { SelectField } from '../../core/ui/SelectField';
import { TextField } from '../../core/ui/TextField';
import { TransportButton } from '../../core/ui/TransportButton';
import { RecorderController, recordingDuration } from './RecorderController';
import styles from './RecorderControls.module.css';

const sampleRates = [44100, 48000, 88200, 96000, 176400, 192000];
const encodings: { value: RecorderEncoding; label: string }[] = [
  { value: 'pcm16', label: '16-bit PCM' },
  { value: 'pcm24', label: '24-bit PCM' },
  { value: 'float32', label: '32-bit float' },
];

export function RecorderControls(props: {
  controlPort: ControlPort;
  graphId: string;
  nodeId: string;
  expectedRevision: number;
  poweredOn?: boolean;
  enabled: boolean;
  channels: number;
  onChannelsChange: (channels: number) => void;
}) {
  const { t } = useTranslation();
  const controller = useMemo(
    () => new RecorderController(props.controlPort, props.graphId, props.nodeId),
    [props.controlPort, props.graphId, props.nodeId],
  );
  useEffect(() => {
    controller.start();
    return () => controller.dispose();
  }, [controller]);
  const snapshot = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
  const settings = snapshot.settings;
  const { opened: optionsOpened, toggle: toggleOptions } = useNodeOptionsState();
  const locked = snapshot.status?.state === 'recording' || snapshot.status?.state === 'paused';
  const disabled = snapshot.busy || !snapshot.status || props.poweredOn === false;
  const fieldsLocked = disabled || locked || !props.enabled || !settings;
  return (
    <div className={styles.root}>
      <div className={styles.transport}>
        <output
          className={styles.time}
          aria-label={t('recorder.duration')}
          title={
            snapshot.status ? t(`recorder.states.${snapshot.status.state}`) : t('recorder.loading')
          }
        >
          {recordingDuration(snapshot.status)}
        </output>
        <RecorderButtons
          canStart={props.enabled && !!settings && !snapshot.settingsError}
          disabled={disabled}
          status={snapshot.status}
          command={(request) => controller.command(request, props.expectedRevision)}
        />
      </div>
      <RecorderError error={snapshot.error ?? snapshot.settingsError} />
      <NodeOptionsSection
        disabled={props.poweredOn === false}
        label={t('routing.nodes.options')}
        opened={optionsOpened}
        onToggle={toggleOptions}
      >
        <SelectField
          label={t('recorder.channels')}
          value={String(props.channels)}
          disabled={fieldsLocked}
          data={Array.from({ length: 8 }, (_, index) => ({
            value: String(index + 1),
            label: String(index + 1),
          }))}
          onChange={(value) => props.onChannelsChange(Number(value))}
        />
        <TextField
          label={t('recorder.folder')}
          value={settings?.folder ?? ''}
          readOnly
          classNames={{
            root: styles.folderField,
            label: styles.folderLabel,
            wrapper: styles.folderWrapper,
            input: styles.folderPath,
          }}
          rightSectionWidth={28}
          rightSectionPointerEvents="auto"
          rightSection={
            props.controlPort.selectRecordingFolder ? (
              <button
                type="button"
                className={styles.folderButton}
                aria-label={t('recorder.chooseFolder')}
                disabled={fieldsLocked}
                onClick={() =>
                  controller.chooseFolder(t('recorder.chooseFolder'), props.expectedRevision)
                }
              >
                <svg aria-hidden="true" viewBox="0 0 20 20">
                  <path d="M2 6V4h6l2 2h8v3M2 6v11h14l2-8H5l-3 8" />
                </svg>
              </button>
            ) : null
          }
        />
        <SelectField
          label={t('recorder.sampleRate')}
          value={String((locked ? snapshot.status?.sampleRate : settings?.sampleRate) ?? 'auto')}
          disabled={fieldsLocked}
          data={[
            { value: 'auto', label: t('recorder.sourceRate') },
            ...recordingRateOptions(locked ? snapshot.status?.sampleRate : settings?.sampleRate),
          ]}
          onChange={(value) =>
            controller.saveSettings(
              { sampleRate: value === 'auto' ? null : Number(value) },
              props.expectedRevision,
            )
          }
        />
        <EncodingField
          value={(locked ? snapshot.status?.encoding : settings?.encoding) ?? 'pcm16'}
          disabled={fieldsLocked}
          onChange={(encoding) => controller.saveSettings({ encoding }, props.expectedRevision)}
        />
      </NodeOptionsSection>
    </div>
  );
}

function RecorderError({ error }: { error: string | null }) {
  const { t } = useTranslation();
  return error ? (
    <div className={styles.error} role="alert">
      {t('recorder.failed')}: {error}
    </div>
  ) : null;
}

function recordingRateOptions(observed: number | null | undefined) {
  return [...new Set([...sampleRates, ...(observed ? [observed] : [])])].map((rate) => ({
    value: String(rate),
    label: `${rate / 1000} kHz`,
  }));
}

function EncodingField(props: {
  value: RecorderEncoding;
  disabled: boolean;
  onChange: (value: RecorderEncoding) => void;
}) {
  const { t } = useTranslation();
  return (
    <SelectField
      {...props}
      label={t('recorder.encoding')}
      data={encodings}
      onChange={(value) => {
        const selected = encodings.find((item) => item.value === value);
        if (selected) props.onChange(selected.value);
      }}
    />
  );
}

function RecorderButtons({
  canStart,
  disabled,
  status,
  command,
}: {
  canStart: boolean;
  disabled: boolean;
  status: RecorderStatus | null;
  command: (request: RecorderRequest) => void;
}) {
  const { t } = useTranslation();
  const locked = status?.state === 'recording' || status?.state === 'paused';
  return (
    <div className={styles.actions}>
      <TransportButton
        icon={status?.state === 'paused' ? 'resume' : 'record'}
        label={status?.state === 'paused' ? t('recorder.resume') : t('recorder.record')}
        active={status?.state === 'recording'}
        disabled={disabled || status?.state === 'recording' || (!locked && !canStart)}
        onClick={() => command({ action: 'record' })}
      />
      <TransportButton
        icon="pause"
        label={t('recorder.pause')}
        disabled={disabled || status?.state !== 'recording'}
        onClick={() => command({ action: 'pause' })}
      />
      <TransportButton
        icon="stop"
        label={t('recorder.stop')}
        disabled={disabled || !locked}
        onClick={() => command({ action: 'stop' })}
      />
    </div>
  );
}
