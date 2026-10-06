import {
  ActionIcon,
  Image,
  Paper,
  Select,
  Slider,
  TextInput,
  Tooltip,
  UnstyledButton,
} from '@mantine/core';
import { useResizeObserver } from '@mantine/hooks';
import {
  type CSSProperties,
  type ReactNode,
  type Ref,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';

import { useAudioMeterLevels } from '../audio-meter/useAudioMeterLevels';
import styles from './AudioNodeCard.module.css';
import {
  type AudioNodeFaceMode,
  audioNodeFaceGeometry,
  channelRowMetrics,
} from './audioNodeLayout';
import { Meter } from './Meter';
import { NodeDisclosureMeasureContext, NodeOptionsSection } from './NodeOptionsSection';
import { useNodeOptionsState } from './nodeOptionsState';
import { ProcessorTimingLabel } from './ProcessorTimingLabel';
import type { StatusTone } from './StatusBadge';
import type { NativeStreamFormat, NativeDeviceFormats } from '../control-port/types';
import type { NativeDirection } from '../driver/DriverPort';
import { nativeFormatKey, nativeFormatLabel } from '../driver/nativeFormatLabel';

const portIds = ['one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight'];
const channelOptions = (
  current: number,
  allowed = Array.from({ length: 64 }, (_, index) => index + 1),
) =>
  [...new Set([...allowed, current])]
    .sort((a, b) => a - b)
    .map((channels) => ({
      label: `${channels}`,
      value: `${channels}`,
    }));

export interface EndpointControl {
  nativeFormats?: NativeDeviceFormats;
  nativeFormatLabels?: { render: string; capture: string; unknown: string };
  formatError?: string | null;
  formatsDisabled?: boolean;
  observedRate?: { label: string; value: number };
  sampleRateOptions?: number[];
  rateError?: string | null;
  canRename?: boolean;
  allowedRenderChannels?: number[];
  allowedCaptureChannels?: number[];
  captureChannels: number;
  captureChannelsLabel: string;
  displayName: string;
  nameLabel: string;
  renderChannels: number;
  renderChannelsLabel: string;
}

export interface EndpointUpdate {
  sampleRate?: number;
  nativeFormat?: { direction: NativeDirection; format: NativeStreamFormat };
  captureChannels: number;
  displayName: string;
  renderChannels: number;
}

export interface AudioNodeCardProps {
  headerActions?: ReactNode;
  muteControl?: ReactNode;
  poweredOff?: boolean;
  processingTimeNs?: number;
  content?: ReactNode;
  bypassed?: boolean;
  /** Device presence only; selection and configuration remain available. */
  offline?: boolean;
  category: string;
  channelCount?: number;
  detail?: string;
  endpointControl?: EndpointControl;
  faceHeight?: number;
  faceMode?: AudioNodeFaceMode;
  onFaceHeightChange?: (height: number) => void;
  onContentHeightChange?: (height: number, disclosureHeight: number) => void;
  gainControl?: { label: string; valueDb: number };
  icon: string;
  iconUrl?: string;
  inputCount: number;
  meterBanks?: readonly AudioNodeMeterBank[];
  outputCount: number;
  optionsLabel?: string;
  parameterControls?: readonly {
    id: string;
    label: string;
    maximum: number;
    minimum: number;
    step: number;
    suffix: string;
    value: number;
  }[];
  processorEditor?: {
    actionLabel: string;
    failureLabel?: string;
    state: 'unavailable' | 'closed' | 'opening' | 'open' | 'failed';
  };
  showPorts?: boolean;
  selected?: boolean;
  status?: string;
  statusTone?: StatusTone;
  title: string;
  onSelect: () => void;
  onGainChange?: (gainDb: number) => void;
  onEndpointChange?: (value: EndpointUpdate) => void;
  onLayoutChange?: () => void;
  onParameterChange?: (parameterId: string, value: number) => void;
  onProcessorEditorRetry?: () => void;
}

export interface AudioNodeMeterBank {
  channels: number;
  label?: string;
  nodeId: string;
  portId: string;
}

function meterBanksForCompactFace(
  faceMode: AudioNodeFaceMode,
  meterBanks: readonly AudioNodeMeterBank[],
) {
  return faceMode === 'compact' ? meterBanks : [];
}

function toggleNodeOptions(toggle: () => void, onLayoutChange?: () => void) {
  toggle();
  requestAnimationFrame(() => onLayoutChange?.());
}

function cssHeight(value: string) {
  const pixels = Number.parseFloat(value);
  return Number.isFinite(pixels) ? pixels : null;
}

function borderBoxHeight(element: HTMLElement) {
  const style = getComputedStyle(element);
  const height = cssHeight(style.height);
  if (height === null) return null;
  if (style.boxSizing === 'border-box') return height;
  return (
    height +
    (cssHeight(style.paddingTop) ?? 0) +
    (cssHeight(style.paddingBottom) ?? 0) +
    (cssHeight(style.borderTopWidth) ?? 0) +
    (cssHeight(style.borderBottomWidth) ?? 0)
  );
}

function useCardMeasurements({
  onFaceHeightChange,
  onContentHeightChange,
}: Pick<AudioNodeCardProps, 'onFaceHeightChange' | 'onContentHeightChange'>) {
  const [faceRef, faceBounds] = useResizeObserver<HTMLButtonElement>();
  const [contentRef, contentBounds] = useResizeObserver<HTMLDivElement>();
  const controlsNode = useRef<HTMLDivElement | null>(null);
  const measureRef = useCallback(
    (element: HTMLDivElement | null) => {
      controlsNode.current = element;
      contentRef(element);
    },
    [contentRef],
  );
  const measureControls = useCallback(() => {
    const controls = controlsNode.current;
    if (!controls) return;
    const total = borderBoxHeight(controls);
    const panel = controls.querySelector<HTMLElement>('[data-node-disclosure]');
    const disclosure = panel ? borderBoxHeight(panel) : 0;
    if (total === null || disclosure === null || disclosure > total) return;
    onContentHeightChange?.(total, disclosure);
  }, [onContentHeightChange]);
  useEffect(() => {
    if (faceBounds.height > 0) onFaceHeightChange?.(faceBounds.height);
  }, [faceBounds.height, onFaceHeightChange]);
  useLayoutEffect(() => {
    // ResizeObserver values trigger a coherent DOM read; they are not mixed as measurements.
    void contentBounds.height;
    void contentBounds.width;
    measureControls();
  }, [contentBounds.height, contentBounds.width, measureControls]);
  return { faceRef, faceBounds, contentRef: measureRef, measureControls };
}

export function AudioNodeCard({
  headerActions,
  muteControl,
  poweredOff = false,
  processingTimeNs,
  bypassed = false,
  offline = false,
  category,
  content,
  channelCount = 0,
  detail,
  endpointControl,
  faceHeight,
  faceMode = 'standard',
  onFaceHeightChange,
  onContentHeightChange,
  gainControl,
  icon,
  iconUrl,
  inputCount,
  meterBanks = [],
  outputCount,
  optionsLabel = 'Options',
  onSelect,
  onEndpointChange,
  onGainChange,
  onLayoutChange,
  onParameterChange,
  onProcessorEditorRetry,
  parameterControls = [],
  processorEditor,
  selected = false,
  showPorts = true,
  status,
  statusTone = 'neutral',
  title,
}: AudioNodeCardProps) {
  const { faceRef, faceBounds, contentRef, measureControls } = useCardMeasurements({
    onFaceHeightChange,
    onContentHeightChange,
  });
  const { opened: optionsOpened, toggle } = useNodeOptionsState();
  const { hasGainControl, hasEndpointControl, hasParameterControls } = nodeOptionsAvailability({
    gainControl,
    onGainChange,
    endpointControl,
    onEndpointChange,
    parameterControls,
    onParameterChange,
    processorEditor,
  });
  const levelMute = hasGainControl ? undefined : muteControl;
  const hasOptions = hasGainControl || hasEndpointControl || hasParameterControls;
  return (
    <Paper
      className={styles.root}
      data-bypassed={bypassed || undefined}
      data-offline={offline || undefined}
      data-powered-off={poweredOff}
      data-endpoint-options={hasEndpointControl || undefined}
      data-has-options={hasOptions || undefined}
      data-header-actions={Boolean(headerActions)}
      data-metered={meterBanks.length > 0 || undefined}
      data-meter-actions={Boolean(levelMute) || undefined}
      data-inline-controls={Boolean(content) || faceMode === 'channel-rows' || undefined}
      data-face-mode={faceMode}
      data-options-opened={optionsOpened || undefined}
      data-selected={selected || undefined}
      style={audioNodeCardStyle({ faceHeight, faceMode, meterBanks, detail }, faceBounds.height)}
      withBorder
    >
      <NodeDisclosureMeasureContext.Provider value={measureControls}>
        <AudioNodeFace
          processingTimeNs={processingTimeNs}
          faceRef={faceRef}
          category={category}
          channelCount={channelCount}
          hideSummary={Boolean(content) || faceMode !== 'standard'}
          detail={detail}
          icon={icon}
          iconUrl={iconUrl}
          inputCount={inputCount}
          meterBanks={meterBanks}
          faceMode={faceMode}
          offline={offline}
          onSelect={onSelect}
          outputCount={outputCount}
          selected={selected}
          status={status}
          statusTone={statusTone}
          title={title}
        />
        <NodeHeaderActions actions={headerActions} />
        <NodeLevelMute control={levelMute} faceMode={faceMode} placement="face" />
        <AudioNodeControls
          content={content}
          compactMeterBanks={meterBanksForCompactFace(faceMode, meterBanks)}
          faceMode={faceMode}
          measureRef={contentRef}
          offline={offline || poweredOff}
          title={title}
        >
          <NodeLevelMute control={levelMute} faceMode={faceMode} placement="settings" />
          <div inert={poweredOff}>
            <ProcessorEditorOption control={processorEditor} onRetry={onProcessorEditorRetry} />
          </div>
          <NodeOptions
            muteControl={hasGainControl ? muteControl : undefined}
            disabled={poweredOff}
            endpointControl={endpointControl}
            gainControl={gainControl}
            hasOptions={hasOptions}
            label={optionsLabel}
            onEndpointChange={onEndpointChange}
            onGainChange={onGainChange}
            onParameterChange={onParameterChange}
            onToggle={() => toggleNodeOptions(toggle, onLayoutChange)}
            opened={optionsOpened}
            parameterControls={hasParameterControls ? parameterControls : []}
          />
        </AudioNodeControls>
        <NodePortOverlays inputCount={inputCount} outputCount={outputCount} visible={showPorts} />
      </NodeDisclosureMeasureContext.Provider>
    </Paper>
  );
}

function NodeHeaderActions({ actions }: { actions?: ReactNode }) {
  return actions ? <div className={styles.headerActions}>{actions}</div> : null;
}

function nodeOptionsAvailability(
  props: Pick<
    AudioNodeCardProps,
    | 'gainControl'
    | 'onGainChange'
    | 'endpointControl'
    | 'onEndpointChange'
    | 'parameterControls'
    | 'onParameterChange'
    | 'processorEditor'
  >,
) {
  return {
    hasGainControl: Boolean(props.gainControl && props.onGainChange),
    hasEndpointControl: Boolean(props.endpointControl && props.onEndpointChange),
    hasParameterControls:
      Boolean(props.parameterControls?.length && props.onParameterChange) &&
      !prefersProcessorEditor(props.processorEditor),
  };
}

function NodeLevelMute({
  control,
  faceMode,
  placement,
}: {
  control: ReactNode;
  faceMode: AudioNodeFaceMode;
  placement: 'face' | 'settings';
}) {
  const compact = faceMode === 'compact';
  if (!control || compact !== (placement === 'settings')) return null;
  return <div className={compact ? styles.levelActions : styles.meterActions}>{control}</div>;
}

function AudioNodeControls({
  content,
  compactMeterBanks,
  faceMode,
  measureRef,
  offline,
  title,
  children,
}: {
  content: ReactNode;
  compactMeterBanks: readonly AudioNodeMeterBank[];
  faceMode: AudioNodeFaceMode;
  measureRef: Ref<HTMLDivElement>;
  offline: boolean;
  title: string;
  children: ReactNode;
}) {
  // Native surfaces stretch with the node. Only intrinsic settings constrain its minimum height.
  const compact = faceMode === 'compact';
  if (compact && compactMeterBanks.length > 0) {
    return (
      <div className={styles.controls}>
        <div ref={measureRef} className={styles.settings}>
          <NodeMeterBanks banks={compactMeterBanks} offline={offline} title={title} />
          {children}
        </div>
        {content ? <div className={`${styles.content} nodrag nopan nowheel`}>{content}</div> : null}
      </div>
    );
  }
  return (
    <div ref={compact ? undefined : measureRef} className={styles.controls}>
      {content ? <div className={`${styles.content} nodrag nopan nowheel`}>{content}</div> : null}
      <div ref={compact ? measureRef : undefined} className={styles.settings}>
        {children}
      </div>
    </div>
  );
}

function audioNodeCardStyle(
  {
    faceHeight,
    faceMode,
    meterBanks,
    detail,
  }: Pick<AudioNodeCardProps, 'faceHeight' | 'faceMode' | 'meterBanks' | 'detail'>,
  renderedFaceHeight: number,
): CSSProperties {
  const geometry = audioNodeFaceGeometry({
    meterBanks,
    hasDetail: Boolean(detail),
    mode: faceMode,
  });
  const height = Math.max(faceHeight ?? 0, geometry.height);
  return {
    '--audio-node-face-height': `${height}px`,
    '--audio-node-rendered-face-height': `${renderedFaceHeight > 0 ? renderedFaceHeight : height}px`,
    ...(faceMode === 'channel-rows'
      ? {
          '--audio-node-header-height': `${channelRowMetrics.headerHeight}px`,
          '--audio-node-body-padding': `${channelRowMetrics.bodyPadding}px`,
          '--audio-node-meter-margin': `${channelRowMetrics.meterMargin}px`,
          '--audio-node-row-height': `${channelRowMetrics.rowHeight}px`,
          '--audio-node-row-gap': `${channelRowMetrics.rowGap}px`,
        }
      : {}),
  } as CSSProperties;
}

function AudioNodeFace(
  props: Pick<
    AudioNodeCardProps,
    | 'category'
    | 'processingTimeNs'
    | 'channelCount'
    | 'detail'
    | 'faceMode'
    | 'icon'
    | 'iconUrl'
    | 'inputCount'
    | 'meterBanks'
    | 'onSelect'
    | 'offline'
    | 'outputCount'
    | 'selected'
    | 'status'
    | 'statusTone'
    | 'title'
  > & { hideSummary?: boolean; faceRef?: Ref<HTMLButtonElement> },
) {
  const {
    processingTimeNs,
    category,
    channelCount = 0,
    hideSummary,
    detail,
    icon,
    iconUrl,
    inputCount,
    meterBanks = [],
    faceMode = 'standard',
    onSelect,
    offline,
    outputCount,
    selected,
    status,
    statusTone,
    title,
  } = props;
  return (
    <UnstyledButton
      ref={props.faceRef}
      aria-label={[title, category, detail, status].filter(Boolean).join(', ')}
      aria-pressed={selected}
      className={styles.button}
      onClick={onSelect}
    >
      <span className={styles.topLine}>
        <span className={styles.icon} aria-hidden="true">
          <NodeIcon icon={icon} iconUrl={iconUrl} />
        </span>
        <span className={styles.category}>{title}</span>
        <ProcessorTimingLabel nanoseconds={processingTimeNs} />
        <NodeStatus status={status} tone={statusTone} />
      </span>
      <span className={styles.body}>
        {detail ? <small className={styles.detail}>{detail}</small> : null}
        {faceMode !== 'compact' && meterBanks.length > 0 ? (
          <NodeMeterBanks banks={meterBanks} offline={Boolean(offline)} title={title} />
        ) : null}
      </span>
      {!hideSummary ? (
        <span className={styles.ioSummary} aria-hidden="true">
          <span>{inputCount > 0 ? `IN ${inputCount}` : ''}</span>
          <span>{channelCount > 0 ? `${channelCount} CH` : ''}</span>
          <span>{outputCount > 0 ? `OUT ${outputCount}` : ''}</span>
        </span>
      ) : null}
    </UnstyledButton>
  );
}

function NodeIcon({ icon, iconUrl }: { icon: string; iconUrl?: string }) {
  const [failedUrl, setFailedUrl] = useState<string>();
  if (iconUrl && iconUrl !== failedUrl) {
    return (
      <Image
        alt=""
        className={styles.iconImage}
        src={iconUrl}
        onError={() => setFailedUrl(iconUrl)}
      />
    );
  }
  const kind = icon.toUpperCase();
  if (kind === 'APP') {
    return (
      <svg aria-hidden="true" className={styles.iconGlyph} viewBox="0 0 16 16">
        <rect height="11" width="12" x="2" y="2.5" />
        <path d="M2.5 5.5h11" />
        <circle cx="4.25" cy="4" r=".45" />
      </svg>
    );
  }
  if (kind === 'GAIN' || kind === 'MIX') {
    return (
      <svg aria-hidden="true" className={styles.iconGlyph} viewBox="0 0 16 16">
        <path d="M3 3v10M8 3v10M13 3v10" />
        <path d="M1.5 6h3M6.5 10h3M11.5 7.5h3" />
      </svg>
    );
  }
  if (kind === 'ADC' || kind === 'MIC') {
    return (
      <svg aria-hidden="true" className={styles.iconGlyph} viewBox="0 0 16 16">
        <rect height="8" width="5" x="5.5" y="2" />
        <path d="M3.5 7.5a4.5 4.5 0 0 0 9 0M8 12v2M5.5 14h5" />
      </svg>
    );
  }
  if (kind === 'DAC') {
    return (
      <svg aria-hidden="true" className={styles.iconGlyph} viewBox="0 0 16 16">
        <path d="M2.5 6h3l3-2.5v9L5.5 10h-3zM11 6a3 3 0 0 1 0 4M12.5 4.5a5 5 0 0 1 0 7" />
      </svg>
    );
  }
  return (
    <svg aria-hidden="true" className={styles.iconGlyph} viewBox="0 0 16 16">
      <rect height="10" width="12" x="2" y="3" />
      <circle cx="6" cy="8" r="1" />
      <circle cx="10" cy="8" r="1" />
    </svg>
  );
}

function NodeStatus({ status, tone }: { status?: string; tone?: StatusTone }) {
  if (!status) return null;
  return (
    <Tooltip label={status} openDelay={350} position="left">
      <span aria-label={status} className={styles.statusDot} data-tone={tone} role="img" />
    </Tooltip>
  );
}

function NodeOptions({
  muteControl,
  disabled,
  endpointControl,
  gainControl,
  hasOptions,
  label,
  onEndpointChange,
  onGainChange,
  onParameterChange,
  onToggle,
  opened,
  parameterControls,
}: {
  muteControl?: ReactNode;
  disabled: boolean;
  endpointControl?: EndpointControl;
  gainControl?: { label: string; valueDb: number };
  hasOptions: boolean;
  label: string;
  onEndpointChange?: AudioNodeCardProps['onEndpointChange'];
  onGainChange?: AudioNodeCardProps['onGainChange'];
  onParameterChange?: AudioNodeCardProps['onParameterChange'];
  onToggle: () => void;
  opened: boolean;
  parameterControls: NonNullable<AudioNodeCardProps['parameterControls']>;
}) {
  if (!hasOptions) return null;
  const controls = (
    <>
      {endpointControl && onEndpointChange ? (
        <div inert={disabled && !endpointControl.nativeFormats}>
          <EndpointOptions control={endpointControl} onChange={onEndpointChange} />
        </div>
      ) : null}
      {gainControl && onGainChange ? (
        <NodeGainControl
          control={gainControl}
          onChange={onGainChange}
          disabled={disabled}
          muteControl={muteControl}
        />
      ) : null}
      {onParameterChange && parameterControls.length > 0 ? (
        <div className={styles.parameterList} inert={disabled}>
          {parameterControls.map((control) => (
            <NodeParameterControl
              control={control}
              key={control.id}
              onChange={(value) => onParameterChange(control.id, value)}
            />
          ))}
        </div>
      ) : null}
    </>
  );
  return parameterControls.length > 0 ? (
    <div className={`${styles.optionsPanel} nodrag nopan nowheel`}>{controls}</div>
  ) : (
    <NodeOptionsSection label={label} opened={opened} onToggle={onToggle}>
      {controls}
    </NodeOptionsSection>
  );
}

function prefersProcessorEditor(control: AudioNodeCardProps['processorEditor']) {
  return Boolean(control && control.state !== 'failed' && control.state !== 'unavailable');
}

function ProcessorEditorOption({
  control,
  onRetry,
}: {
  control?: AudioNodeCardProps['processorEditor'];
  onRetry?: AudioNodeCardProps['onProcessorEditorRetry'];
}) {
  if (!control || !onRetry || (control.state !== 'failed' && control.state !== 'closed'))
    return null;
  return (
    <div className={`${styles.processorEditor} nodrag nopan nowheel`}>
      <ActionIcon
        aria-label={control.actionLabel}
        className={styles.processorEditorButton}
        data-state={control.state}
        onClick={(event) => {
          event.stopPropagation();
          onRetry();
        }}
        size={24}
        variant="subtle"
      >
        <svg
          aria-hidden="true"
          width="14"
          height="14"
          viewBox="0 0 16 16"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.25"
        >
          {control.state === 'closed' ? (
            <path d="M6 3H3v10h10v-3M9 3h4v4M13 3 7 9" />
          ) : (
            <path d="M13 7a5 5 0 1 0-1 4M13 2v5H8" />
          )}
        </svg>
      </ActionIcon>
      {control.failureLabel ? (
        <span role="status" className={styles.processorEditorFailure}>
          {control.failureLabel}
        </span>
      ) : null}
    </div>
  );
}

function NodeParameterControl({
  control,
  onChange,
}: {
  control: NonNullable<AudioNodeCardProps['parameterControls']>[number];
  onChange: (value: number) => void;
}) {
  const [value, setValue] = useState(control.value);
  useEffect(() => setValue(control.value), [control.value]);
  return (
    <div className={`${styles.parameterControl} nodrag nopan nowheel`}>
      <span className={styles.parameterLabel}>{control.label}</span>
      <span className={styles.parameterValue}>
        {value.toFixed(control.step < 1 ? 1 : 0)} {control.suffix}
      </span>
      <Slider
        className={styles.parameterSlider}
        label={null}
        max={control.maximum}
        min={control.minimum}
        onClick={(event) => event.stopPropagation()}
        onChange={(nextValue) => {
          setValue(nextValue);
          onChange(nextValue);
        }}
        onPointerDown={(event) => event.stopPropagation()}
        step={control.step}
        thumbLabel={control.label}
        value={value}
      />
    </div>
  );
}

function NodePortOverlays({
  inputCount,
  outputCount,
  visible,
}: {
  inputCount: number;
  outputCount: number;
  visible: boolean;
}) {
  if (!visible) return null;
  return (
    <>
      <NodePorts count={inputCount} side="input" />
      <NodePorts count={outputCount} side="output" />
    </>
  );
}

interface EndpointOptionsProps {
  control: EndpointControl;
  onChange: (value: EndpointUpdate) => void;
}

function EndpointOptions({ control, onChange }: EndpointOptionsProps) {
  const [displayName, setDisplayName] = useState(control.displayName);
  useEffect(() => setDisplayName(control.displayName), [control.displayName]);
  const commitName = () => {
    if (control.canRename === false) return;
    const normalized = displayName.trim();
    if (!normalized) {
      setDisplayName(control.displayName);
      return;
    }
    if (normalized !== control.displayName) {
      onChange(endpointValue(control, { displayName: normalized }));
    }
  };
  return (
    <div className={styles.endpointOptions}>
      <TextInput
        readOnly={control.canRename === false}
        className={styles.optionField}
        label={control.nameLabel}
        maxLength={40}
        onBlur={commitName}
        onChange={(event) => setDisplayName(event.currentTarget.value)}
        onKeyDown={(event) => {
          if (event.key === 'Enter') event.currentTarget.blur();
        }}
        size="xs"
        value={displayName}
      />
      <div className={styles.endpointChannels}>
        <Select
          allowDeselect={false}
          className={styles.optionField}
          data={channelOptions(control.renderChannels, control.allowedRenderChannels)}
          disabled={
            control.allowedRenderChannels !== undefined && control.allowedRenderChannels.length < 2
          }
          label={control.renderChannelsLabel}
          onChange={(value) => {
            if (value)
              onChange(endpointValue(control, { displayName, renderChannels: Number(value) }));
          }}
          size="xs"
          value={`${control.renderChannels}`}
        />
        <Select
          allowDeselect={false}
          className={styles.optionField}
          data={channelOptions(control.captureChannels, control.allowedCaptureChannels)}
          disabled={
            control.allowedCaptureChannels !== undefined &&
            control.allowedCaptureChannels.length < 2
          }
          label={control.captureChannelsLabel}
          onChange={(value) => {
            if (value) {
              onChange(endpointValue(control, { captureChannels: Number(value), displayName }));
            }
          }}
          size="xs"
          value={`${control.captureChannels}`}
        />
      </div>
      {control.observedRate && control.sampleRateOptions?.length ? (
        <Select
          allowDeselect={false}
          className={styles.optionField}
          label={control.observedRate.label}
          error={control.rateError}
          data={[...new Set([...control.sampleRateOptions, control.observedRate.value])].map(
            (value) => ({ value: String(value), label: `${value / 1000} kHz` }),
          )}
          value={String(control.observedRate.value)}
          onChange={(value) => {
            if (value) onChange({ ...endpointValue(control, {}), sampleRate: Number(value) });
          }}
          size="xs"
        />
      ) : control.observedRate ? (
        <TextInput
          readOnly
          className={styles.optionField}
          label={control.observedRate.label}
          size="xs"
          value={`${control.observedRate.value / 1000} kHz`}
        />
      ) : null}
      {control.nativeFormats && control.nativeFormatLabels ? (
        <NativeEndpointFormats control={control} onChange={onChange} />
      ) : null}
    </div>
  );
}

function NativeEndpointFormats({ control, onChange }: EndpointOptionsProps) {
  return <>
    {(['render', 'capture'] as const).map((direction) => {
      const formats = control.nativeFormats?.[direction];
      if (!formats) return null;
      if (!formats.current) return <TextInput key={direction} readOnly size="xs"
        label={control.nativeFormatLabels?.[direction]} value={control.nativeFormatLabels?.unknown} />;
      const data = [...formats.supported];
      if (!data.some((format) => nativeFormatKey(format) === nativeFormatKey(formats.current!))) data.push(formats.current);
      return <Select key={direction} allowDeselect={false} size="xs"
        className={styles.optionField} label={control.nativeFormatLabels?.[direction]}
        error={control.formatError}
        disabled={control.formatsDisabled || formats.supported.length === 0}
        value={nativeFormatKey(formats.current)}
        data={data.map((format) => ({ value: nativeFormatKey(format),
          label: nativeFormatLabel(format) }))}
        onChange={(value) => {
          const format = formats.supported.find((candidate) => nativeFormatKey(candidate) === value);
          if (format) onChange({ ...endpointValue(control, {}), nativeFormat: { direction, format } });
        }} />;
    })}
    {control.formatError ? <p role="alert">{control.formatError}</p> : null}
  </>;
}

function endpointValue(
  control: EndpointControl,
  update: Partial<Pick<EndpointControl, 'captureChannels' | 'displayName' | 'renderChannels'>>,
) {
  return {
    captureChannels: update.captureChannels ?? control.captureChannels,
    displayName: update.displayName ?? control.displayName,
    renderChannels: update.renderChannels ?? control.renderChannels,
  };
}

function NodeGainControl({
  control,
  onChange,
  disabled,
  muteControl,
}: {
  control: { label: string; valueDb: number };
  onChange: (gainDb: number) => void;
  disabled: boolean;
  muteControl?: ReactNode;
}) {
  const [value, setValue] = useState(control.valueDb);
  useEffect(() => setValue(control.valueDb), [control.valueDb]);
  return (
    <span
      className={`${styles.gainControl} nodrag nopan nowheel`}
      data-mute-control={Boolean(muteControl)}
    >
      <span className={styles.gainValue}>{value.toFixed(1)} dB</span>
      <Slider
        aria-label={control.label}
        disabled={disabled}
        className={styles.gainSlider}
        label={null}
        max={12}
        min={-60}
        onClick={(event) => event.stopPropagation()}
        onChange={(nextValue) => {
          setValue(nextValue);
          onChange(nextValue);
        }}
        onPointerDown={(event) => event.stopPropagation()}
        step={0.5}
        thumbLabel={control.label}
        value={value}
      />
      {muteControl}
    </span>
  );
}

function NodeMeterBanks({
  banks,
  offline,
  title,
}: {
  banks: readonly AudioNodeMeterBank[];
  offline: boolean;
  title: string;
}) {
  const visibleBanks = banks.filter((bank) => bank.channels > 0);
  if (visibleBanks.length === 0) return null;
  return (
    <span className={styles.meterGroups}>
      {visibleBanks.map((bank) => (
        <MeterBank
          bank={bank}
          key={JSON.stringify([bank.nodeId, bank.portId])}
          offline={offline}
          showLabel={visibleBanks.length > 1}
          title={title}
        />
      ))}
    </span>
  );
}

function MeterBank({
  bank,
  offline,
  showLabel,
  title,
}: {
  bank: AudioNodeMeterBank;
  offline: boolean;
  showLabel: boolean;
  title: string;
}) {
  const levels = useAudioMeterLevels(bank.nodeId, bank.portId);
  return (
    <span className={styles.meterGroup} data-meter-port={bank.portId}>
      {showLabel && bank.label ? <span className={styles.meterBankLabel}>{bank.label}</span> : null}
      <NodeMeters
        channels={bank.channels}
        levels={offline ? [] : levels}
        meterLabel={bank.label ? `${title} ${bank.label}` : title}
      />
    </span>
  );
}

function NodeMeters({
  channels,
  levels,
  meterLabel,
}: {
  channels: number;
  levels: readonly number[];
  meterLabel: string;
}) {
  const visibleChannels = Math.min(64, Math.max(1, Math.trunc(channels)));
  const channelNumbers = Array.from({ length: visibleChannels }, (_, index) => index + 1);
  if (visibleChannels > 8) {
    return (
      <span
        className={styles.meterBank}
        data-channels={visibleChannels}
        data-density={visibleChannels > 32 ? 'dense' : 'compact'}
      >
        {channelNumbers.map((channel) => (
          <span className={styles.meterCell} key={`channel-${channel}`}>
            <Meter
              label={`${meterLabel} channel ${channel}`}
              level={levels[channel - 1] ?? 0}
              orientation="vertical"
            />
            <span aria-hidden="true" className={styles.meterIndex}>
              {meterIndexLabel(channel, visibleChannels)}
            </span>
          </span>
        ))}
      </span>
    );
  }
  return (
    <span className={styles.meters} data-channels={visibleChannels}>
      {channelNumbers.map((channel) => (
        <span className={styles.meterRow} key={`channel-${channel}`}>
          <span aria-hidden="true">{meterChannelLabel(channel, visibleChannels)}</span>
          <Meter
            label={`${meterLabel} channel ${channel}`}
            level={levels[channel - 1] ?? 0}
            orientation="horizontal"
          />
        </span>
      ))}
    </span>
  );
}

function meterChannelLabel(channel: number, channels: number) {
  if (channels === 2) return channel === 1 ? 'L' : 'R';
  return `${channel}`;
}

function meterIndexLabel(channel: number, channels: number) {
  return channel === 1 || channel === channels || channel % 8 === 0 ? channel : '';
}

function NodePorts({ count, side }: { count: number; side: 'input' | 'output' }) {
  return (
    <span
      aria-hidden="true"
      className={`${styles.ports} ${side === 'input' ? styles.inputPorts : styles.outputPorts}`}
    >
      {portIds.slice(0, count).map((portId, portIndex) => (
        <span className={styles.port} data-port-index={portIndex} key={`${side}-${portId}`} />
      ))}
    </span>
  );
}
