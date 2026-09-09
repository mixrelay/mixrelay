import { useEffect, useMemo, useState, useSyncExternalStore } from 'react';
import { useTranslation } from 'react-i18next';
import type {
  ControlPort,
  RecorderEncoding,
  RecorderRequest,
  RecorderStatus,
} from '../../core/control-port/types';
import { ActionButton } from '../../core/ui/ActionButton';
import { NodeOptionsSection } from '../../core/ui/NodeOptionsSection';
import { SelectField } from '../../core/ui/SelectField';
import { TextField } from '../../core/ui/TextField';
import { TransportButton } from '../../core/ui/TransportButton';
import { absoluteRecordingPath, RecorderController, recordingDuration } from './RecorderController';
import styles from './RecorderControls.module.css';
import { useRecordingPath } from './useRecordingPath';

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
  const picker = useRecordingPath(props.controlPort, snapshot.status);
  const { path, setPath } = picker;
  const [sampleRate, setSampleRate] = useState('auto');
  const [encoding, setEncoding] = useState<RecorderEncoding>('float32');
  const [optionsOpened, setOptionsOpened] = useState(false);
  const locked = snapshot.status?.state === 'recording' || snapshot.status?.state === 'paused';
  const disabled = snapshot.busy || picker.busy || !snapshot.status;
  const fieldsLocked = disabled || locked || !props.enabled;
  const displayPath = locked ? (snapshot.status?.path ?? '') : path;
  return (
    <div className={styles.root}>
      <div className={styles.transport}>
        <RecorderButtons
          canStart={props.enabled}
          disabled={disabled}
          status={snapshot.status}
          command={controller.command}
          settings={{
            path,
            sampleRate: sampleRate === 'auto' ? null : Number(sampleRate),
            encoding,
          }}
        />
        <output
          className={styles.time}
          aria-label={t('recorder.duration')}
          title={
            snapshot.status ? t(`recorder.states.${snapshot.status.state}`) : t('recorder.loading')
          }
        >
          {recordingDuration(snapshot.status)}
        </output>
      </div>
      <RecorderError error={snapshot.error ?? picker.error} />
      <NodeOptionsSection
        label={t('routing.nodes.options')}
        opened={optionsOpened}
        onToggle={() => setOptionsOpened((opened) => !opened)}
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
          label={t('recorder.path')}
          value={displayPath}
          disabled={fieldsLocked}
          onChange={(event) => setPath(event.currentTarget.value)}
        />
        {props.controlPort.selectRecordingPath ? (
          <ActionButton
            size="xs"
            variant="default"
            disabled={fieldsLocked}
            onClick={() => picker.choose(t('recorder.choosePath'))}
          >
            {t('recorder.choosePath')}
          </ActionButton>
        ) : null}
        <SelectField
          label={t('recorder.sampleRate')}
          value={locked ? String(snapshot.status?.sampleRate ?? 'auto') : sampleRate}
          disabled={fieldsLocked}
          data={[
            { value: 'auto', label: t('recorder.sourceRate') },
            ...recordingRateOptions(snapshot.status?.sampleRate),
          ]}
          onChange={setSampleRate}
        />
        <EncodingField
          value={locked ? (snapshot.status?.encoding ?? encoding) : encoding}
          disabled={fieldsLocked}
          onChange={setEncoding}
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
  settings,
}: {
  canStart: boolean;
  disabled: boolean;
  status: RecorderStatus | null;
  command: (request: RecorderRequest) => void;
  settings: { path: string; sampleRate: number | null; encoding: RecorderEncoding };
}) {
  const { t } = useTranslation();
  const locked = status?.state === 'recording' || status?.state === 'paused';
  return (
    <div className={styles.actions}>
      <TransportButton
        icon={status?.state === 'paused' ? 'resume' : 'record'}
        label={status?.state === 'paused' ? t('recorder.resume') : t('recorder.record')}
        active={status?.state === 'recording'}
        disabled={
          disabled ||
          status?.state === 'recording' ||
          (!locked && !canStart) ||
          (!locked && !absoluteRecordingPath(settings.path))
        }
        onClick={() =>
          command(
            status?.state === 'paused'
              ? { action: 'record' }
              : {
                  action: 'record',
                  ...settings,
                },
          )
        }
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
