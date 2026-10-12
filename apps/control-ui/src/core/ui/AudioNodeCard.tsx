import { type CSSProperties, type ReactNode, type Ref, useEffect, useState } from 'react';
import { Button } from 'react-aria-components';
import { useTranslation } from 'react-i18next';
import styles from './AudioNodeCard.module.css';
import {
  AudioNodeEndpointOptions,
  type EndpointControl,
  type EndpointUpdate,
} from './AudioNodeEndpointOptions';
import { AudioNodeIcon } from './AudioNodeIcon';
import { type AudioNodeMeterBank, AudioNodeMeterBanks } from './AudioNodeMeterBanks';
import { type AudioNodeParameterControl, AudioNodeParameterGrid } from './AudioNodeParameterGrid';
import {
  type AudioNodeFaceMode,
  audioNodeFaceGeometry,
  audioNodeFrame,
  channelRowMetrics,
} from './audioNodeLayout';
import { DynamicsMeter, type DynamicsMeterContract } from './DynamicsMeter';
import { GraphicEqualizer, type GraphicEqualizerContract } from './GraphicEqualizer';
import { IconActionButton } from './IconActionButton';
import {
  NodeDisclosureMeasureContext,
  NodeHeaderOptionsContext,
  NodeOptionsSection,
} from './NodeOptionsSection';
import { useNodeOptionsState } from './nodeOptionsState';
import { ProcessorTimingLabel } from './ProcessorTimingLabel';
import type { StatusTone } from './StatusBadge';
import { StatusIndicator } from './StatusIndicator';
import { Tooltip } from './Tooltip';
import { useCardMeasurements } from './useAudioNodeCardMeasurements';
import { useElementBounds } from './useElementBounds';
import { useNodeHeaderFocus } from './useNodeHeaderFocus';
import { ValueSlider } from './ValueSlider';

const portIds = ['one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight'];
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
  parameterControls?: readonly AudioNodeParameterControl[];
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
  optionsLabel,
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
  const { t } = useTranslation();
  const resolvedOptionsLabel = optionsLabel ?? t('routing.nodes.options');
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
              label={`${title}: ${resolvedOptionsLabel}`}
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
          <AudioNodeMeterBanks banks={compactMeterBanks} offline={offline} title={title} />
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
          <AudioNodeIcon icon={icon} iconUrl={iconUrl} />
        </span>
        <span className={styles.category} data-node-title>
          {title}
        </span>
        <ProcessorTimingLabel nanoseconds={processingTimeNs} />
        {status ? (
          <span className={styles.statusSlot}>
            <StatusIndicator label={status} tone={statusTone} position="left" />
          </span>
        ) : null}
      </span>
      <span className={styles.body}>
        {hideSummary && detail ? <small className={styles.detail}>{detail}</small> : null}
        {faceMode !== 'compact' && meterBanks.length > 0 ? (
          <AudioNodeMeterBanks banks={meterBanks} offline={Boolean(offline)} title={title} />
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
          <AudioNodeEndpointOptions control={endpointControl} onChange={onEndpointChange} />
        </div>
      ) : null}
      {gainControl && onGainChange ? (
        <NodeGainControl control={gainControl} onChange={onGainChange} disabled={disabled} />
      ) : null}
      {onParameterChange && parameterControls.length > 0 && !equalizer ? (
        <AudioNodeParameterGrid
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
          <AudioNodeParameterGrid
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
