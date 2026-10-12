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
import { Button } from 'react-aria-components';
import { useAudioMeterLevels } from '../audio-meter/useAudioMeterLevels';
import type { NativeDeviceFormats, NativeSampleFormat } from '../control-port/types';
import { sharedChannelCounts } from '../driver/sharedDeviceFormat';
import styles from './AudioNodeCard.module.css';
import {
  type AudioNodeFaceMode,
  audioNodeFaceGeometry,
  audioNodeFrame,
  channelRowMetrics,
} from './audioNodeLayout';
import { DynamicsMeter, type DynamicsMeterContract } from './DynamicsMeter';
import { GraphicEqualizer, type GraphicEqualizerContract } from './GraphicEqualizer';
import { IconActionButton } from './IconActionButton';
import { Meter } from './Meter';
import { type EndpointBufferControl, NativeEndpointBuffer } from './NativeEndpointBuffer';
import { NativeSampleFormatField } from './NativeSampleFormatField';
import {
  NodeDisclosureMeasureContext,
  NodeHeaderOptionsContext,
  NodeOptionsSection,
} from './NodeOptionsSection';
import { useNodeOptionsState } from './nodeOptionsState';
import { ParameterField } from './ParameterField';
import { ProcessorTimingLabel } from './ProcessorTimingLabel';
import { SelectField } from './SelectField';
import type { StatusTone } from './StatusBadge';
import { TextField } from './TextField';
import { Tooltip } from './Tooltip';
import { useElementBounds } from './useElementBounds';
import { useNodeHeaderFocus } from './useNodeHeaderFocus';
import { ValueSlider } from './ValueSlider';

const portIds = ['one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight'];
const channelOptions = (current: number, allowed: number[]) =>
  [...new Set([...allowed, current])]
    .sort((a, b) => a - b)
    .map((channels) => ({
      label: `${channels}`,
      value: `${channels}`,
      disabled: !allowed.includes(channels),
    }));

export interface EndpointControl {
  bufferControl?: EndpointBufferControl;
  nativeFormats?: NativeDeviceFormats;
  nativeFormatLabels?: { bitDepth: string; unknown: string; mixed?: string };
  channelsLabel?: string;
  formatError?: string | null;
  formatsDisabled?: boolean;
  observedRate?: { label: string; value: number };
  sampleRateOptions?: number[];
  rateError?: string | null;
  canRename?: boolean;
  allowedRenderChannels?: number[];
  allowedCaptureChannels?: number[];
  captureChannels: number;
  displayName: string;
  nameLabel: string;
  renderChannels: number;
}

export interface EndpointUpdate {
  bufferFrames?: number;
  sampleRate?: number;
  sampleFormat?: NativeSampleFormat;
  captureChannels: number;
  displayName: string;
  renderChannels: number;
}

export interface AudioNodeCardProps {
  equalizer?: GraphicEqualizerContract;
  onParametersChange?: (updates: readonly { id: string; value: number }[]) => void;
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
    description?: string;
    section?: 'main' | 'options';
    maximum: number;
    minimum: number;
    step: number;
    suffix: string;
    value: number;
    scale?: 'linear' | 'logarithmic';
    choices?: readonly { value: number; label: string; description?: string }[];
    precise?: boolean;
    presentation?: 'slider' | 'knob';
    group?: { id: string; label: string };
    endpoints?: { minimum: string; maximum: string };
  }[];
  dynamics?: DynamicsMeterContract;
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
  /** Omit when a parent graph owns node selection and keyboard focus. */
  onSelect?: () => void;
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
  const [faceRef, faceBounds] = useElementBounds<HTMLElement>();
  const [contentRef, contentBounds] = useElementBounds<HTMLDivElement>();
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
  equalizer,
  dynamics,
  onParametersChange,
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
  const [headerOptionsTarget, setHeaderOptionsTarget] = useState<HTMLDivElement | null>(null);
  const [headerRef, headerBounds] = useElementBounds<HTMLDivElement>();
  const { hasOptions, hasEndpointControl, hasParameterControls } = nodeOptionsAvailability({
    gainControl,
    onGainChange,
    endpointControl,
    onEndpointChange,
    parameterControls,
    onParameterChange,
    processorEditor,
  });
  const editor = { title, control: processorEditor, onRetry: onProcessorEditorRetry };
  return (
    <div
      className={`${styles.root} nokey`}
      data-equalizer={Boolean(equalizer) || undefined}
      {...cardStateAttributes({ bypassed, offline, poweredOff, selected, dynamics })}
      data-endpoint-options={hasEndpointControl || undefined}
      data-has-options={hasOptions || undefined}
      data-header-actions={Boolean(headerActions)}
      data-metered={meterBanks.length > 0 || undefined}
      data-inline-controls={Boolean(content) || faceMode === 'channel-rows' || undefined}
      data-face-mode={faceMode}
      data-options-opened={optionsOpened || undefined}
      style={cardRootStyle({ faceHeight, faceMode, meterBanks, faceBounds, headerBounds })}
    >
      <NodeDisclosureMeasureContext.Provider value={measureControls}>
        <NodeHeaderOptionsContext.Provider value={headerOptionsTarget}>
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
            meterBanks={dynamics ? [] : meterBanks}
            faceMode={faceMode}
            offline={offline}
            onSelect={onSelect}
            outputCount={outputCount}
            selected={selected}
            status={status}
            statusTone={statusTone}
            title={title}
          />
          <NodeHeaderActions
            {...{ title, actions: headerActions, mute: muteControl }}
            bypassControls={parameterControls}
            bypassDisabled={poweredOff}
            onBypassChange={onParameterChange}
            headerRef={headerRef}
            targetRef={setHeaderOptionsTarget}
          />
          <AudioNodeControls
            content={content}
            compactMeterBanks={dynamics ? [] : meterBanksForCompactFace(faceMode, meterBanks)}
            dynamics={dynamics}
            parameterControls={parameterControls}
            bypassed={bypassed}
            faceMode={faceMode}
            measureRef={contentRef}
            offline={offline || poweredOff}
            title={title}
          >
            <div inert={poweredOff}>
              <ProcessorEditorOption {...editor} />
            </div>
            <NodeOptions
              disabled={poweredOff}
              endpointControl={endpointControl}
              gainControl={gainControl}
              hasOptions={hasOptions}
              label={`${title}: ${optionsLabel}`}
              onEndpointChange={onEndpointChange}
              onGainChange={onGainChange}
              onParameterChange={onParameterChange}
              onToggle={() => toggleNodeOptions(toggle, onLayoutChange)}
              opened={optionsOpened}
              parameterControls={hasParameterControls ? parameterControls : []}
              equalizer={equalizer}
              onParametersChange={onParametersChange}
            />
          </AudioNodeControls>
          <NodePortOverlays inputCount={inputCount} outputCount={outputCount} visible={showPorts} />
        </NodeHeaderOptionsContext.Provider>
      </NodeDisclosureMeasureContext.Provider>
    </div>
  );
}

function cardStateAttributes({
  bypassed,
  offline,
  poweredOff,
  selected,
  dynamics,
}: Pick<AudioNodeCardProps, 'bypassed' | 'offline' | 'poweredOff' | 'selected' | 'dynamics'>) {
  return {
    'data-bypassed': bypassed || undefined,
    'data-offline': offline || undefined,
    'data-powered-off': poweredOff,
    'data-selected': selected || undefined,
    'data-dynamics': Boolean(dynamics) || undefined,
  };
}

function cardRootStyle(
  props: Parameters<typeof audioNodeCardStyle>[0] & {
    faceBounds: { height: number };
    headerBounds: { width: number };
  },
): CSSProperties {
  return {
    ...audioNodeCardStyle(props, props.faceBounds.height),
    '--audio-node-header-actions-inset': `${props.headerBounds.width + 12}px`,
  } as CSSProperties;
}

function NodeHeaderActions({
  title,
  actions,
  mute,
  bypassControls,
  bypassDisabled,
  onBypassChange,
  headerRef,
  targetRef,
}: {
  title: string;
  actions?: ReactNode;
  mute?: ReactNode;
  bypassControls: NonNullable<AudioNodeCardProps['parameterControls']>;
  bypassDisabled: boolean;
  onBypassChange?: AudioNodeCardProps['onParameterChange'];
  headerRef: Ref<HTMLDivElement>;
  targetRef: Ref<HTMLDivElement>;
}) {
  const focus = useNodeHeaderFocus();
  return (
    <div className={styles.headerActions} ref={headerRef} {...focus}>
      <div className={styles.headerSlot}>{actions}</div>
      <div className={styles.headerSlot}>{mute}</div>
      <div className={styles.headerSlot}>
        <BuiltinBypass
          title={title}
          controls={bypassControls}
          disabled={bypassDisabled}
          onChange={onBypassChange}
        />
      </div>
      <div className={styles.headerSlot} ref={targetRef} />
    </div>
  );
}

function BuiltinBypass({
  title,
  controls,
  disabled,
  onChange,
}: {
  title: string;
  controls: NonNullable<AudioNodeCardProps['parameterControls']>;
  disabled: boolean;
  onChange: AudioNodeCardProps['onParameterChange'];
}) {
  const control = controls.find((item) => item.id === 'bypass');
  if (!control || !onChange) return null;
  const label = `${title}: ${control.label}: ${control.choices?.find((choice) => choice.value === control.value)?.label ?? control.value}`;
  return (
    <IconActionButton
      label={label}
      aria-pressed={Boolean(control.value)}
      disabled={disabled}
      onClick={() => onChange(control.id, control.value ? 0 : 1)}
    >
      <svg viewBox="0 0 20 20" aria-hidden="true">
        <path d="M2 10h4m8 0h4M6 6h8v8H6z" />
        {control.value ? <path d="m4 16 12-12" /> : null}
      </svg>
    </IconActionButton>
  );
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
  const hasGainControl = Boolean(props.gainControl && props.onGainChange);
  const hasEndpointControl = Boolean(props.endpointControl && props.onEndpointChange);
  const hasParameterControls =
    Boolean(props.parameterControls?.length && props.onParameterChange) &&
    !prefersProcessorEditor(props.processorEditor);
  return {
    hasOptions: hasGainControl || hasEndpointControl || hasParameterControls,
    hasEndpointControl,
    hasParameterControls,
  };
}

function AudioNodeControls({
  content,
  compactMeterBanks,
  dynamics,
  parameterControls,
  bypassed,
  faceMode,
  measureRef,
  offline,
  title,
  children,
}: {
  content: ReactNode;
  compactMeterBanks: readonly AudioNodeMeterBank[];
  dynamics?: DynamicsMeterContract;
  parameterControls: NonNullable<AudioNodeCardProps['parameterControls']>;
  bypassed: boolean;
  faceMode: AudioNodeFaceMode;
  measureRef: Ref<HTMLDivElement>;
  offline: boolean;
  title: string;
  children: ReactNode;
}) {
  // Native surfaces stretch with the node. Only intrinsic settings constrain its minimum height.
  const compact = faceMode === 'compact';
  const dynamicsMeter = dynamics ? (
    <DynamicsMeter
      contract={dynamics}
      controls={parameterControls}
      disabled={offline}
      bypassed={bypassed}
    />
  ) : null;
  if (compact && compactMeterBanks.length > 0) {
    return (
      <div className={styles.controls} data-node-controls="">
        <div ref={measureRef} className={styles.settings}>
          <NodeMeterBanks banks={compactMeterBanks} offline={offline} title={title} />
          {dynamicsMeter}
          {children}
        </div>
        {content ? <div className={`${styles.content} nodrag nopan nowheel`}>{content}</div> : null}
      </div>
    );
  }
  return (
    <div ref={compact ? undefined : measureRef} className={styles.controls} data-node-controls="">
      {content ? <div className={`${styles.content} nodrag nopan nowheel`}>{content}</div> : null}
      <div ref={compact ? measureRef : undefined} className={styles.settings}>
        {dynamicsMeter}
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
  }: Pick<AudioNodeCardProps, 'faceHeight' | 'faceMode' | 'meterBanks'>,
  renderedFaceHeight: number,
): CSSProperties {
  const geometry = audioNodeFaceGeometry({
    meterBanks,
    mode: faceMode,
  });
  const height = Math.max(faceHeight ?? 0, geometry.height);
  return {
    '--audio-node-face-height': `${height}px`,
    '--audio-node-header-height': `${audioNodeFrame.headerHeight}px`,
    '--audio-node-utility-height': `${audioNodeFrame.utilityHeight}px`,
    '--audio-node-border-width': `${audioNodeFrame.borderWidth}px`,
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
  > & { hideSummary?: boolean; faceRef?: (element: HTMLElement | null) => void },
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
  const Face = onSelect ? 'button' : 'div';
  return (
    <Face
      type={onSelect ? 'button' : undefined}
      ref={props.faceRef as Ref<HTMLButtonElement & HTMLDivElement>}
      data-node-face=""
      aria-label={
        onSelect ? [title, category, detail, status].filter(Boolean).join(', ') : undefined
      }
      aria-pressed={onSelect ? selected : undefined}
      className={styles.button}
      onClick={onSelect}
    >
      <span className={styles.topLine}>
        <span className={styles.icon} aria-hidden="true">
          <NodeIcon icon={icon} iconUrl={iconUrl} />
        </span>
        <span className={styles.category} data-node-title>
          {title}
        </span>
        <ProcessorTimingLabel nanoseconds={processingTimeNs} />
        <NodeStatus status={status} tone={statusTone} />
      </span>
      <span className={styles.body}>
        {hideSummary && detail ? <small className={styles.detail}>{detail}</small> : null}
        {faceMode !== 'compact' && meterBanks.length > 0 ? (
          <NodeMeterBanks banks={meterBanks} offline={Boolean(offline)} title={title} />
        ) : null}
      </span>
      {!hideSummary ? (
        <span className={styles.ioSummary}>
          <span aria-hidden="true">
            {inputCount > 0 && outputCount > 0 && inputCount !== outputCount
              ? `IN ${inputCount} / OUT ${outputCount}`
              : `${Math.max(channelCount, inputCount, outputCount)} CH`}
          </span>
          {detail ? <small className={styles.detail}>{detail}</small> : null}
        </span>
      ) : null}
    </Face>
  );
}

function NodeIcon({ icon, iconUrl }: { icon: string; iconUrl?: string }) {
  const [failedUrl, setFailedUrl] = useState<string>();
  if (iconUrl && iconUrl !== failedUrl) {
    return (
      <img
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
    <Tooltip nonInteractive label={status} position="left">
      <span aria-label={status} className={styles.statusDot} data-tone={tone} role="img" />
    </Tooltip>
  );
}

function NodeOptions({
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
  equalizer,
  onParametersChange,
}: {
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
  equalizer?: GraphicEqualizerContract;
  onParametersChange?: AudioNodeCardProps['onParametersChange'];
}) {
  if (!hasOptions) return null;
  if (equalizer && onParameterChange)
    return (
      <div className={`${styles.optionsPanel} nodrag nopan nowheel`}>
        <GraphicEqualizer
          contract={equalizer}
          controls={parameterControls}
          disabled={disabled}
          bypassed={Boolean(parameterControls.find((control) => control.id === 'bypass')?.value)}
          onChange={onParameterChange}
          onParametersChange={onParametersChange}
          renderValues={(values) => (
            <NodeOptionsSection
              label={label}
              opened={opened}
              onToggle={onToggle}
              disabled={disabled}
            >
              {values}
            </NodeOptionsSection>
          )}
        />
      </div>
    );
  const mainParameters = parameterControls.filter(
    (control) => control.id !== 'bypass' && control.section !== 'options',
  );
  const optionParameters = parameterControls.filter((control) => control.section === 'options');
  const controls = (
    <>
      {endpointControl && onEndpointChange ? (
        <div inert={disabled && !endpointControl.nativeFormats}>
          <EndpointOptions control={endpointControl} onChange={onEndpointChange} />
        </div>
      ) : null}
      {gainControl && onGainChange ? (
        <NodeGainControl control={gainControl} onChange={onGainChange} disabled={disabled} />
      ) : null}
      {onParameterChange && parameterControls.length > 0 && !equalizer ? (
        <NodeParameterFields
          controls={mainParameters}
          disabled={disabled}
          onChange={onParameterChange}
        />
      ) : null}
    </>
  );
  return parameterControls.length > 0 ? (
    <>
      <div className={`${styles.optionsPanel} nodrag nopan nowheel`}>{controls}</div>
      {onParameterChange && optionParameters.length > 0 ? (
        <NodeOptionsSection label={label} opened={opened} onToggle={onToggle} disabled={disabled}>
          <NodeParameterFields
            controls={optionParameters}
            disabled={disabled}
            onChange={onParameterChange}
          />
        </NodeOptionsSection>
      ) : null}
    </>
  ) : (
    <NodeOptionsSection label={label} opened={opened} onToggle={onToggle}>
      {controls}
    </NodeOptionsSection>
  );
}

function NodeParameterFields({
  controls,
  disabled,
  onChange,
}: {
  controls: NonNullable<AudioNodeCardProps['parameterControls']>;
  disabled: boolean;
  onChange: NonNullable<AudioNodeCardProps['onParameterChange']>;
}) {
  return (
    <div className={styles.parameterList} inert={disabled}>
      {parameterControlGroups(controls).map((group) => (
        <div
          key={group.id}
          className={group.knobs ? styles.knobGroup : undefined}
          data-columns={Math.min(3, group.controls.length)}
        >
          {group.label ? <span className={styles.parameterGroupLabel}>{group.label}</span> : null}
          {group.controls.map((control) => (
            <NodeParameterControl
              control={control}
              key={control.id}
              disabled={disabled}
              onChange={(value) => onChange(control.id, value)}
            />
          ))}
        </div>
      ))}
    </div>
  );
}

function prefersProcessorEditor(control: AudioNodeCardProps['processorEditor']) {
  return Boolean(control && control.state !== 'failed' && control.state !== 'unavailable');
}

function ProcessorEditorOption({
  title,
  control,
  onRetry,
}: {
  title: string;
  control?: AudioNodeCardProps['processorEditor'];
  onRetry?: AudioNodeCardProps['onProcessorEditorRetry'];
}) {
  if (!control || !onRetry || (control.state !== 'failed' && control.state !== 'closed'))
    return null;
  return (
    <div className={`${styles.processorEditor} nodrag nopan nowheel`}>
      <Tooltip label={`${title}: ${control.actionLabel}`}>
        <Button
          aria-label={`${title}: ${control.actionLabel}`}
          className={styles.processorEditorButton}
          data-state={control.state}
          onClick={(event) => {
            event.stopPropagation();
            onRetry();
          }}
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
        </Button>
      </Tooltip>
      {control.failureLabel ? (
        <span role="status" className={styles.processorEditorFailure}>
          {control.failureLabel}
        </span>
      ) : null}
    </div>
  );
}

function parameterControlGroups(controls: NonNullable<AudioNodeCardProps['parameterControls']>) {
  const groups: {
    id: string;
    label?: string;
    knobs: boolean;
    controls: (typeof controls)[number][];
  }[] = [];
  for (const control of controls) {
    const id =
      control.presentation === 'knob' && !control.choices
        ? (control.group?.id ?? 'knobs')
        : control.id;
    const last = groups.at(-1);
    if (last?.id === id) last.controls.push(control);
    else
      groups.push({
        id,
        label: control.group?.label,
        knobs: control.presentation === 'knob' && !control.choices,
        controls: [control],
      });
  }
  return groups;
}

function NodeParameterControl({
  control,
  onChange,
  disabled,
}: {
  control: NonNullable<AudioNodeCardProps['parameterControls']>[number];
  onChange: (value: number) => void;
  disabled?: boolean;
}) {
  const [value, setValue] = useState(control.value);
  useEffect(() => setValue(control.value), [control.value]);
  if (control.precise || control.choices || control.scale) {
    return <ParameterField control={control} onChange={onChange} disabled={disabled} />;
  }
  return (
    <div className={`${styles.parameterControl} nodrag nopan nowheel`}>
      <span className={styles.parameterLabel}>{control.label}</span>
      <span className={styles.parameterValue}>
        {value.toFixed(control.step < 1 ? 1 : 0)} {control.suffix}
      </span>
      <ValueSlider
        disabled={disabled}
        max={control.maximum}
        min={control.minimum}
        onChange={(nextValue) => {
          setValue(nextValue);
          onChange(nextValue);
        }}
        step={control.step}
        label={control.label}
        value={value}
        valueText={`${value} ${control.suffix}`}
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
      <TextField
        readOnly={control.canRename === false}
        label={control.nameLabel}
        maxLength={40}
        onBlur={commitName}
        onChange={(event) => setDisplayName(event.currentTarget.value)}
        onKeyDown={(event) => {
          if (event.key === 'Enter') event.currentTarget.blur();
        }}
        value={displayName}
      />
      <EndpointChannels control={control} displayName={displayName} onChange={onChange} />
      <EndpointSampleRate control={control} onChange={onChange} />
      {control.nativeFormats && control.nativeFormatLabels ? (
        <NativeEndpointFormats
          control={control}
          formats={control.nativeFormats}
          labels={control.nativeFormatLabels}
          onChange={onChange}
        />
      ) : null}
      {control.bufferControl ? (
        <NativeEndpointBuffer
          control={control.bufferControl}
          onChange={(bufferFrames) => onChange({ ...endpointValue(control, {}), bufferFrames })}
        />
      ) : null}
    </div>
  );
}

function EndpointChannels({
  control,
  displayName,
  onChange,
}: EndpointOptionsProps & { displayName: string }) {
  const hasRender = control.renderChannels > 0;
  const hasCapture = control.captureChannels > 0;
  const current =
    hasRender && hasCapture && control.renderChannels !== control.captureChannels
      ? null
      : hasRender
        ? control.renderChannels
        : control.captureChannels;
  const allowed = sharedChannelCounts(
    control.allowedRenderChannels ?? Array.from({ length: 64 }, (_, index) => index + 1),
    control.allowedCaptureChannels ?? Array.from({ length: 64 }, (_, index) => index + 1),
    hasRender,
    hasCapture,
  );
  return (
    <SelectField
      label={control.channelsLabel ?? 'Channels'}
      placeholder={control.nativeFormatLabels?.mixed ?? 'Mixed'}
      data={channelOptions(current ?? 0, allowed).filter((option) => option.value !== '0')}
      value={current === null ? null : String(current)}
      disabled={allowed.length === 0 || (allowed.length === 1 && allowed[0] === current)}
      onChange={(value) => {
        // The select disables observed counts outside the allowed device capability.
        onChange(
          endpointValue(control, {
            displayName,
            renderChannels: hasRender ? Number(value) : 0,
            captureChannels: hasCapture ? Number(value) : 0,
          }),
        );
      }}
    />
  );
}

function EndpointSampleRate({ control, onChange }: EndpointOptionsProps) {
  return (
    <>
      {control.observedRate && control.sampleRateOptions?.length ? (
        <SelectField
          label={control.observedRate.label}
          error={control.rateError}
          data={[...new Set([...control.sampleRateOptions, control.observedRate.value])].map(
            (value) => ({ value: String(value), label: `${value / 1000} kHz` }),
          )}
          value={String(control.observedRate.value)}
          disabled={
            control.sampleRateOptions.length === 1 &&
            control.sampleRateOptions[0] === control.observedRate.value
          }
          onChange={(value) => {
            onChange({ ...endpointValue(control, {}), sampleRate: Number(value) });
          }}
        />
      ) : control.observedRate ? (
        <TextField
          readOnly
          label={control.observedRate.label}
          value={`${control.observedRate.value / 1000} kHz`}
        />
      ) : null}
    </>
  );
}

function NativeEndpointFormats({
  control,
  formats,
  labels,
  onChange,
}: EndpointOptionsProps & {
  formats: NativeDeviceFormats;
  labels: NonNullable<EndpointControl['nativeFormatLabels']>;
}) {
  return (
    <NativeSampleFormatField
      formats={formats}
      render={control.renderChannels > 0}
      capture={control.captureChannels > 0}
      label={labels.bitDepth}
      unknownLabel={labels.unknown}
      mixedLabel={labels.mixed}
      disabled={control.formatsDisabled}
      error={control.formatError}
      onChange={(sampleFormat) => onChange({ ...endpointValue(control, {}), sampleFormat })}
    />
  );
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
}: {
  control: { label: string; valueDb: number };
  onChange: (gainDb: number) => void;
  disabled: boolean;
}) {
  const [value, setValue] = useState(control.valueDb);
  useEffect(() => setValue(control.valueDb), [control.valueDb]);
  return (
    <span className={`${styles.gainControl} nodrag nopan nowheel`}>
      <span className={styles.gainValue}>{value.toFixed(1)} dB</span>
      <ValueSlider
        disabled={disabled}
        max={12}
        min={-60}
        onChange={(nextValue) => {
          setValue(nextValue);
          onChange(nextValue);
        }}
        step={0.5}
        label={control.label}
        value={value}
        valueText={`${value} dB`}
      />
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
