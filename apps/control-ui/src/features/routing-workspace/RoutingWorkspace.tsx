import type { TFunction } from 'i18next';
import {
  type ReactNode,
  type Ref,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { useTranslation } from 'react-i18next';
import {
  AudioGraphCanvas,
  type AudioGraphChangeKind,
  type ProcessorEditorLabels,
} from '../../core/audio-graph/AudioGraphCanvas';
import type { AudioGraphModel, AudioGraphNode } from '../../core/audio-graph/model';
import { audioGraphNodeTitle, normalizeAudioGraphNodeSize } from '../../core/audio-graph/model';
import type {
  AudioDeviceDescriptor,
  AudioSessionStatus,
  CapturableApplication,
  ControlPort,
  ExternalProcessorDescriptor,
  FailedProcessorInstance,
  RuntimeGraphDocument,
  VirtualEndpointRateError,
  VirtualEndpointSettings,
  VirtualEndpointSpec,
} from '../../core/control-port/types';
import { ControlPortError } from '../../core/control-port/types';
import { type HistoryStep, useUndoHistory } from '../../core/history/useUndoHistory';
import { usePaidFeatures } from '../../core/licensing/FeatureAccess';
import { ActionButton } from '../../core/ui/ActionButton';
import type { EndpointControl } from '../../core/ui/AudioNodeEndpointOptions';
import { InlineNotice } from '../../core/ui/InlineNotice';
import { NodeMuteButton, NodePowerButton } from '../../core/ui/NodeSignalControls';
import { hasPlatformModifier, isNativeTextEntryTarget } from '../../core/ui/platformShortcut';
import { RecoveryActions } from '../../core/ui/RecoveryActions';
import { RetryAlert } from '../../core/ui/RetryAlert';
import { TitleField } from '../../core/ui/TitleField';
import { useRemovedFocusReturn } from '../../core/ui/useRemovedFocusReturn';
import { useSustainedProblem } from '../../core/ui/useSustainedProblem';
import { WorkspaceHeader } from '../../core/ui/WorkspaceHeader';
import { RecorderControls } from '../recorder/RecorderControls';
import { applicationInventorySelection } from './applicationIdentity';
import { defaultEffectLabels, isChainKind, isEffectKind } from './builtinEffects';
import { controlChanges, rebaseControlsWithHistory } from './controlReconciliation';
import {
  changeEndpointBuffer,
  type DeviceBufferAction,
  presentEndpointBuffers,
} from './endpointBuffers';
import { presentEqualizerSampleRate } from './equalizerPresentation';
import { GraphApplicationQueue } from './GraphApplicationQueue';
import { graphHistoryGroup } from './graphHistoryGroup';
import { presentHardwareNodeStatus } from './hardwareNodeStatus';
import {
  addRoutingNode,
  audioGraphToRuntimeGraph,
  changedProcessorParameters,
  type GraphPresentation,
  presentRoutingApplicationIcons,
  presentVirtualEndpointSettings,
  type RoutingNodeRequest,
  runtimeGraphToAudioGraph,
  updateRecorderChannels,
  updateRoutingNodeGain,
  updateRoutingNodeParameter,
  updateRoutingNodeParameters,
  updateVirtualEndpoint,
  type VirtualEndpointUpdate,
} from './model';
import { ProcessorCatalogAlert, ProcessorEditorAlerts } from './ProcessorControlAlerts';
import { ProcessorParameterAlert } from './ProcessorParameterAlert';
import { ProcessorRecoveryControl } from './ProcessorRecoveryControl';
import { processorCatalogPlaceholder, processorSelectionForMenuItem } from './processorCatalogMenu';
import { presentProcessorLayouts, useProcessorLayoutChange } from './processorLayouts';
import { presentProcessorTimings } from './processorTiming';
import styles from './RoutingWorkspace.module.css';
import { rebaseGraphEdit } from './rebaseGraphEdit';
import {
  ADD_PROCESSOR_DIRECTORY_ITEM_ID,
  audioInputItemId,
  audioOutputItemId,
  createNodeMenuItems,
  RESCAN_PROCESSORS_ITEM_ID,
} from './routingNodeMenus';
import { useGraphLayout } from './useGraphLayout';
import { useNodeControls } from './useNodeControls';
import { presentNodeDisplayNames, useNodeDisplayNameEditor } from './useNodeDisplayNameEditor';
import { useProcessorCatalog } from './useProcessorCatalog';
import { useProcessorEditors } from './useProcessorEditors';
import { useProcessorParameterPublisher } from './useProcessorParameterPublisher';
import { useRoutingInventory } from './useRoutingInventory';
import { useRoutingMeters } from './useRoutingMeters';
import { useVirtualEndpointCreation } from './useVirtualEndpointCreation';
import { presentApplicationReselection, presentVirtualNodeStatus } from './virtualNodeStatus';

type WorkspaceState = 'active' | 'applying' | 'loading' | 'unavailable' | 'blocked';
type UpdateModel = (model: AudioGraphModel, kind: AudioGraphChangeKind) => void;
const GRAPH_RECOVERY_MS = 500;

export interface RoutingWorkspaceProps {
  deviceBufferAction?: DeviceBufferAction;
  deviceDriverAction?: {
    settings?: VirtualEndpointSettings;
    enabled: boolean;
    error: { endpointId: string | null; message: string } | null;
    change: (endpointId: string, update: VirtualEndpointUpdate) => void;
    create: (spec: VirtualEndpointSpec) => Promise<VirtualEndpointSpec | null>;
  };
  headerActions?: ReactNode;
  notice?: ReactNode;
  failedProcessors?: FailedProcessorInstance[];
  catalogRevision?: number;
  processorTimings?: import('../../core/control-port/types').ProcessorTiming[];
  deviceRateAction?: {
    change: (endpointId: string, sampleRate: number) => void;
    enabled: boolean;
    error: VirtualEndpointRateError | null;
  };
  graphId?: string;
  interactionDisabled?: boolean;
  sessionDocument?: RuntimeGraphDocument;
  audioSession?: AudioSessionStatus;
  playbackPaused?: boolean;
  controlPort: ControlPort;
  onBusyChange?: (busy: boolean) => void;
  onTitleChange?: (title: string) => void;
  title?: string;
}

function useGraphPresentation(): GraphPresentation {
  const { t } = useTranslation();
  return useMemo<GraphPresentation>(
    () => ({
      effectLabels: Object.fromEntries(
        Object.keys(defaultEffectLabels).map((key) => [key, t(`effects.${key}`)]),
      ),
      application: t('routing.nodes.application'),
      attack: t('routing.nodes.attack'),
      audioInput: t('routing.nodes.audioInput'),
      audioOutput: t('routing.nodes.audioOutput'),
      recorder: t('recorder.title'),
      channels: t('driverManagement.channels'),
      ceiling: t('routing.nodes.ceiling'),
      compressor: t('routing.nodes.compressor'),
      deviceName: t('routing.nodes.deviceName'),
      virtualSourcePort: t('routing.nodes.virtualSourcePort'),
      gain: t('routing.nodes.gain'),
      limiter: t('routing.nodes.limiter'),
      makeupGain: t('routing.nodes.makeupGain'),
      mixer: t('routing.nodes.mixer'),
      options: t('routing.nodes.options'),
      outputGain: t('routing.nodes.outputGain'),
      processor: t('routing.nodes.processor'),
      processorLayoutUnavailable: t('routing.nodes.processorLayoutUnavailable'),
      ports: {
        application: t('routing.ports.application'),
        deviceInput: t('routing.ports.deviceInput'),
        deviceOutput: t('routing.ports.deviceOutput'),
        effectInput: t('routing.ports.effectInput', { name: '{{name}}' }),
        effectOutput: t('routing.ports.effectOutput', { name: '{{name}}' }),
        gainInput: t('routing.ports.gainInput'),
        gainOutput: t('routing.ports.gainOutput'),
        mixerInput: t('routing.ports.mixerInput'),
        mixerOutput: t('routing.ports.mixerOutput'),
      },
      providerBusKinds: {
        main: t('routing.processorLayouts.main'),
        sidechain: t('routing.processorLayouts.sidechain'),
        auxiliary: t('routing.processorLayouts.auxiliary'),
      },
      ratio: t('routing.nodes.ratio'),
      release: t('routing.nodes.release'),
      routing: t('routing.groups.routing'),
      sourceGain: t('routing.nodes.sourceGain'),
      virtualSinkPort: t('routing.nodes.virtualSinkPort'),
      threshold: t('routing.nodes.threshold'),
      unsupported: t('routing.nodes.unsupported'),
      utility: t('routing.groups.utility'),
      virtualInput: t('routing.nodes.virtualInput'),
      virtualOutput: t('routing.nodes.virtualOutput'),
    }),
    [t],
  );
}

interface RoutingNodeCreationOptions {
  tryVirtualAdd: (itemId: string, position: { x: number; y: number }) => boolean;
  audioSession?: AudioSessionStatus;
  applications: readonly CapturableApplication[];
  audioDevices: readonly AudioDeviceDescriptor[];
  model: AudioGraphModel | null;
  processors: readonly ExternalProcessorDescriptor[];
  presentation: GraphPresentation;
  refreshProcessors: (selectDirectory: boolean) => void;
  updateModel: UpdateModel;
}

function useRoutingNodeCreation(options: RoutingNodeCreationOptions) {
  const {
    audioSession,
    applications,
    audioDevices,
    model,
    presentation,
    processors,
    refreshProcessors,
    updateModel,
    tryVirtualAdd,
  } = options;
  return useCallback(
    (itemId: string, position: { x: number; y: number }) => {
      if (tryVirtualAdd(itemId, position)) return;
      if (itemId === ADD_PROCESSOR_DIRECTORY_ITEM_ID) {
        refreshProcessors(true);
        return;
      }
      if (itemId === RESCAN_PROCESSORS_ITEM_ID) {
        refreshProcessors(false);
        return;
      }
      const request = routingNodeRequest(itemId, applications, audioDevices, processors);
      if (!model || !request) return;
      updateModel(
        addRoutingNode(
          model,
          request,
          position,
          presentation,
          audioSession?.virtualEndpointSettings,
        ),
        'runtime',
      );
    },
    [
      applications,
      audioDevices,
      audioSession,
      model,
      presentation,
      processors,
      refreshProcessors,
      updateModel,
      tryVirtualAdd,
    ],
  );
}

function routingNodeRequest(
  itemId: string,
  applications: readonly CapturableApplication[],
  audioDevices: readonly AudioDeviceDescriptor[],
  processors: readonly ExternalProcessorDescriptor[],
): RoutingNodeRequest | null {
  for (const application of applications) {
    const selection = applicationInventorySelection(application);
    if (selection?.id !== itemId) continue;
    return {
      bundleId: application.bundleId,
      kind: 'application',
      selector: selection.selector,
      title: application.displayName,
    };
  }
  const input = audioDevices.find((device) => audioInputItemId(device) === itemId);
  if (input) {
    return {
      channels: supportedChannelCount(input.inputChannels),
      kind: 'audio_input',
      title: input.displayName,
      uid: input.uid,
    };
  }
  const output = audioDevices.find((device) => audioOutputItemId(device) === itemId);
  if (output) {
    return {
      channels: supportedChannelCount(output.outputChannels),
      kind: 'audio_output',
      title: output.displayName,
      uid: output.uid,
    };
  }
  if (itemId === 'compressor') return { kind: 'compressor' };
  if (itemId.startsWith('builtin:') && isEffectKind(itemId.slice(8)))
    return { kind: 'effect', effect: itemId.slice(8) as import('./builtinEffects').EffectKind };
  if (itemId.startsWith('effect-chain:') && isChainKind(itemId.slice(13)))
    return {
      kind: 'effect_chain',
      preset: itemId.slice(13) as import('./builtinEffects').ChainKind,
    };
  if (itemId === 'recorder') return { kind: 'recorder' };
  if (itemId === 'limiter') return { kind: 'limiter' };
  const processor = processorSelectionForMenuItem(itemId, processors);
  if (processor) return { kind: 'provider', ...processor };
  return null;
}

function supportedChannelCount(channels: number): number {
  return Math.max(1, Math.min(64, Math.trunc(channels)));
}

function useRoutingNodeGain(model: AudioGraphModel | null, updateModel: UpdateModel) {
  return useMemo(
    () =>
      model
        ? (nodeId: string, gainDb: number) =>
            updateModel(updateRoutingNodeGain(model, nodeId, gainDb), 'level')
        : undefined,
    [model, updateModel],
  );
}

function useRoutingNodeParameter(model: AudioGraphModel | null, updateModel: UpdateModel) {
  return useMemo(
    () =>
      model
        ? (nodeId: string, parameterId: string, value: number) => {
            updateModel(
              updateRoutingNodeParameter(model, nodeId, parameterId, value),
              parameterId.startsWith('external:')
                ? 'processor_parameter'
                : parameterId === 'preset'
                  ? 'runtime'
                  : 'level',
            );
          }
        : undefined,
    [model, updateModel],
  );
}

function useRoutingNodeParameters(model: AudioGraphModel | null, updateModel: UpdateModel) {
  return useMemo(
    () =>
      model
        ? (nodeId: string, updates: readonly { id: string; value: number }[]) =>
            updateModel(updateRoutingNodeParameters(model, nodeId, updates), 'parameter_group')
        : undefined,
    [model, updateModel],
  );
}

function virtualEndpointId(model: AudioGraphModel | null, nodeId: string) {
  const node = model?.nodes.find((candidate) => candidate.id === nodeId);
  const config = node?.runtime?.type === 'virtual_endpoint' ? node.runtime.config : null;
  return config && typeof config === 'object' && 'id' in config ? config.id : null;
}

function changeEndpointRate(
  endpointId: unknown,
  sampleRate: number,
  action: RoutingWorkspaceProps['deviceRateAction'],
) {
  if (action?.enabled && typeof endpointId === 'string' && endpointId.trim())
    action.change(endpointId, sampleRate);
}

function useVirtualEndpointUpdate(
  model: AudioGraphModel | null,
  presentation: GraphPresentation,
  updateModel: UpdateModel,
  settings?: AudioSessionStatus['virtualEndpointSettings'],
  rateAction?: RoutingWorkspaceProps['deviceRateAction'],
  driverAction?: RoutingWorkspaceProps['deviceDriverAction'],
  bufferAction?: DeviceBufferAction,
) {
  return useMemo(() => {
    if (!model) return undefined;
    return (nodeId: string, update: VirtualEndpointUpdate) => {
      if (update.bufferFrames !== undefined)
        return changeEndpointBuffer(model, nodeId, update.bufferFrames, bufferAction);
      const endpointId = virtualEndpointId(model, nodeId);
      if (driverAction) {
        changeDriverEndpoint(endpointId, update, driverAction);
        return;
      }
      if (update.sampleFormat !== undefined) return;
      if (update.sampleRate !== undefined)
        return changeEndpointRate(endpointId, update.sampleRate, rateAction);
      updateModel(updateVirtualEndpoint(model, nodeId, update, presentation, settings), 'runtime');
    };
  }, [model, presentation, updateModel, settings, rateAction, driverAction, bufferAction]);
}

function changeDriverEndpoint(
  endpointId: unknown,
  update: VirtualEndpointUpdate,
  action: NonNullable<RoutingWorkspaceProps['deviceDriverAction']>,
) {
  if (action.enabled && typeof endpointId === 'string') action.change(endpointId, update);
}

function useWorkspaceGraph(
  controlPort: ControlPort,
  presentation: GraphPresentation,
  processors: readonly ExternalProcessorDescriptor[],
  sessionDocument?: RuntimeGraphDocument,
  graphId?: string,
) {
  const [document, setDocument] = useState<RuntimeGraphDocument | null>(null);
  const [model, setModel] = useState<AudioGraphModel | null>(null);
  const [state, setState] = useState<WorkspaceState>('loading');
  const [errorCode, setErrorCode] = useState<string | null>(null);
  const [persistenceError, setPersistenceError] = useState<string | null>(null);
  const conversion = useRef({ presentation, processors });
  conversion.current = { presentation, processors };
  useEffect(() => {
    // Catalog/localization refreshes update presentation, never fetch over a dirty draft.
    setModel((current) => {
      const source = document;
      if (!current || !source) return current;
      const refreshed = runtimeGraphToAudioGraph(
        audioGraphToRuntimeGraph(current, source),
        presentation,
        processors,
      );
      return {
        ...refreshed,
        nodes: refreshed.nodes.map((node) => {
          // A presentation-only serialization round trip preserves every visible node identity.
          const previous = current.nodes.find((old) => old.id === node.id) as AudioGraphNode;
          return { ...node, position: previous.position, size: previous.size };
        }),
      };
    });
  }, [presentation, processors, document]);
  useEffect(() => {
    if (sessionDocument) return;
    // A replacement port owns a different document. Do not let its faster layout
    // response restore onto the previous engine's model and consume the restore.
    setDocument(null);
    setModel(null);
    setState('loading');
    let cancelled = false;
    void controlPort
      .getGraph()
      .then((nextDocument) => {
        if (cancelled) return;
        setDocument(nextDocument);
        setModel(
          runtimeGraphToAudioGraph(
            nextDocument,
            conversion.current.presentation,
            conversion.current.processors,
          ),
        );
        setState('active');
      })
      .catch(() => {
        if (!cancelled) setState('unavailable');
      });
    return () => {
      cancelled = true;
    };
  }, [controlPort, sessionDocument]);
  return {
    graphId,
    document,
    errorCode,
    persistenceError,
    setPersistenceError,
    model,
    setDocument,
    setErrorCode,
    setModel,
    setState,
    state,
  };
}

function useUnavailableGraphRecovery(
  controlPort: ControlPort,
  presentation: GraphPresentation,
  processors: readonly ExternalProcessorDescriptor[],
  graph: ReturnType<typeof useWorkspaceGraph>,
) {
  const { setDocument, setErrorCode, setModel, setState, state } = graph;
  useEffect(() => {
    if (state !== 'unavailable') return;
    let cancelled = false;
    let retryTimer: ReturnType<typeof setTimeout>;

    const recover = () => {
      void controlPort.getGraph().then(
        (nextDocument) => {
          if (cancelled) return;
          setDocument(nextDocument);
          setModel(runtimeGraphToAudioGraph(nextDocument, presentation, processors));
          setErrorCode(null);
          setState('active');
        },
        () => {
          if (!cancelled) retryTimer = setTimeout(recover, GRAPH_RECOVERY_MS);
        },
      );
    };

    // A graph submission may already have succeeded when its follow-up status request encounters a
    // transient IPC failure. Reloading the authoritative graph keeps an empty graph valid and lets
    // the editor recover without requiring another user edit or an application restart.
    retryTimer = setTimeout(recover, GRAPH_RECOVERY_MS);
    return () => {
      cancelled = true;
      clearTimeout(retryTimer);
    };
  }, [controlPort, presentation, processors, setDocument, setErrorCode, setModel, setState, state]);
}

function runGraphHistoryShortcut(event: KeyboardEvent, undo: () => boolean, redo: () => boolean) {
  const key = event.key.toLowerCase();
  if (key === 'z') return event.shiftKey ? redo() : undo();
  return event.ctrlKey && key === 'y' ? redo() : false;
}

function useGraphHistoryShortcuts(undo: () => boolean, redo: () => boolean, enabled: boolean) {
  useEffect(() => {
    if (!enabled) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (
        event.defaultPrevented ||
        event.altKey ||
        !hasPlatformModifier(event) ||
        isNativeTextEntryTarget(event.target)
      ) {
        return;
      }
      if (runGraphHistoryShortcut(event, undo, redo)) event.preventDefault();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [redo, undo, enabled]);
}

type ProcessorParameters = Pick<
  ReturnType<typeof useProcessorParameterPublisher>,
  'whenIdle' | 'discardFailures'
>;

function rebaseWorkspaceEdit(
  before: RuntimeGraphDocument,
  document: RuntimeGraphDocument,
  local: AudioGraphModel,
  presentation: GraphPresentation,
  processors: readonly ExternalProcessorDescriptor[],
) {
  const merged = rebaseGraphEdit(before, audioGraphToRuntimeGraph(local, before), document);
  const restored = runtimeGraphToAudioGraph(merged, presentation, processors);
  return {
    ...restored,
    nodes: restored.nodes.map((node) => {
      const previous = local.nodes.find((entry) => entry.id === node.id);
      return {
        ...node,
        position: previous?.position ?? node.position,
        size: previous?.size ?? node.size,
      };
    }),
  };
}

function useGraphApplication(
  controlPort: ControlPort,
  presentation: GraphPresentation,
  processors: readonly ExternalProcessorDescriptor[],
  graph: ReturnType<typeof useWorkspaceGraph>,
  history: {
    clear: () => void;
    rebase: (transform: (model: AudioGraphModel) => AudioGraphModel) => void;
  },
  currentModel: { current: AudioGraphModel | null },
  parameters: ProcessorParameters,
  catalogRevision?: number,
  failedProcessors: FailedProcessorInstance[] = [],
) {
  const latest = useRef({
    presentation,
    processors,
    graph,
    history,
    catalogRevision,
    failedProcessors,
  });
  latest.current = { presentation, processors, graph, history, catalogRevision, failedProcessors };
  const queue = useMemo(
    () =>
      new GraphApplicationQueue(controlPort, {
        getGraphId: () => latest.current.graph.graphId,
        getCatalogRevision: () => latest.current.catalogRevision,
        hasFailedProcessor: (graphId, nodeId, instanceId) =>
          latest.current.failedProcessors.some(
            (failed) =>
              failed.graphId === graphId &&
              failed.nodeId === nodeId &&
              failed.instanceId === instanceId,
          ),
        beforeApply: parameters.whenIdle,
        beforeReload: async () => {
          try {
            await parameters.whenIdle();
          } catch (error: unknown) {
            // The failure stays visible until restore succeeds. Only settled
            // parameter failures permit an explicit authoritative reload.
            if (!(error instanceof ControlPortError)) throw error;
          }
        },
        getReloadLayout: controlPort.getGraphLayout?.bind(controlPort),
        getDocument: () => latest.current.graph.document,
        getModel: () => currentModel.current,
        document: (document) => latest.current.graph.setDocument(document),
        error: (code) => latest.current.graph.setErrorCode(code),
        persistenceWarning: (code) => latest.current.graph.setPersistenceError(code),
        state: (state) => latest.current.graph.setState(state),
        rebase: (before, document, local) => {
          const current = latest.current;
          const model = rebaseWorkspaceEdit(
            before,
            document,
            local,
            current.presentation,
            current.processors,
          );
          current.history.clear();
          currentModel.current = model;
          current.graph.setModel(model);
          return model;
        },
        reconcile: (before, document) => {
          const changes = controlChanges(before, document);
          const model = currentModel.current;
          if (!changes || !model) return false;
          const rebased = rebaseControlsWithHistory(model, changes, latest.current.history.rebase);
          const next = { ...rebased, revision: document.revision };
          currentModel.current = next;
          latest.current.graph.setModel(next);
          return true;
        },
        restore: (document, savedLayout) => {
          const current = latest.current;
          const restored = runtimeGraphToAudioGraph(
            document,
            current.presentation,
            current.processors,
          );
          // Reload preserves displayed geometry and uses saved layout for new nodes.
          const model = {
            ...restored,
            nodes: restored.nodes.map((node) => {
              const old = currentModel.current?.nodes.find((entry) => entry.id === node.id);
              return {
                ...node,
                position: old?.position ?? savedLayout?.positions[node.id] ?? node.position,
                size: normalizeAudioGraphNodeSize(
                  node,
                  old ? old.size : savedLayout?.sizes?.[node.id],
                ),
              };
            }),
          };
          currentModel.current = model;
          current.graph.setModel(model);
          current.history.clear();
          parameters.discardFailures();
        },
      }),
    [controlPort, currentModel, parameters.whenIdle, parameters.discardFailures],
  );
  useEffect(() => () => queue.reset(), [queue]);
  return queue;
}

function processorEditIntent(
  before: AudioGraphModel | null,
  after: AudioGraphModel,
  kind: AudioGraphChangeKind,
) {
  return kind === 'processor_parameter' && before ? changedProcessorParameters(before, after) : [];
}

function restoreHistoryModel(
  step: { value: AudioGraphModel; kind: AudioGraphChangeKind },
  current: AudioGraphModel,
) {
  const { value, kind } = step;
  const currentRevision = current.revision;
  return {
    ...value,
    revision: kind === 'layout' ? currentRevision : Math.max(currentRevision, value.revision) + 1,
  };
}

type GraphHistoryStep = HistoryStep<AudioGraphModel, AudioGraphChangeKind>;

function useGraphHistoryNavigation(options: {
  applyModel: GraphApplicationQueue;
  currentModel: { current: AudioGraphModel | null };
  graphRevision: () => number;
  nextRedoStep: () => GraphHistoryStep | null;
  nextUndoStep: () => GraphHistoryStep | null;
  publishProcessorParameter: ReturnType<typeof useProcessorParameterPublisher>['publish'];
  savePositions: (model: AudioGraphModel) => void;
  setModel: (model: AudioGraphModel) => void;
}) {
  const {
    applyModel,
    currentModel,
    graphRevision,
    nextRedoStep,
    nextUndoStep,
    publishProcessorParameter,
    savePositions,
    setModel,
  } = options;
  const restore = useCallback(
    (step: GraphHistoryStep | null) => {
      if (!step) return false;
      // History is created only by a loaded canvas and cleared when its workspace owner changes.
      const previousModel = currentModel.current as AudioGraphModel;
      const restoredModel = restoreHistoryModel(step, previousModel);
      currentModel.current = restoredModel;
      setModel(restoredModel);
      savePositions(restoredModel);
      if (step.kind === 'processor_parameter' && previousModel && applyModel.canPublishParameters) {
        for (const update of changedProcessorParameters(previousModel, restoredModel)) {
          publishProcessorParameter({ ...update, expectedRevision: graphRevision() });
        }
      } else if (step.kind !== 'layout') {
        applyModel.submit(
          restoredModel,
          processorEditIntent(previousModel, restoredModel, step.kind),
        );
      }
      return true;
    },
    [applyModel, currentModel, graphRevision, publishProcessorParameter, savePositions, setModel],
  );
  const undo = useCallback(() => restore(nextUndoStep()), [nextUndoStep, restore]);
  const redo = useCallback(() => restore(nextRedoStep()), [nextRedoStep, restore]);
  return { redo, undo };
}

function liveWorkspaceOwner(current: { active: boolean }, expected: { active: boolean }) {
  return current === expected && expected.active;
}

function gestureChangeKind(kind: AudioGraphChangeKind) {
  return kind === 'level' || kind === 'processor_parameter' || kind === 'parameter_group';
}

function recordGraphEdit(
  history: Pick<
    ReturnType<typeof useUndoHistory<AudioGraphModel, AudioGraphChangeKind>>,
    'record' | 'recordDebounced' | 'flush'
  >,
  before: AudioGraphModel,
  after: AudioGraphModel,
  kind: AudioGraphChangeKind,
) {
  const group = graphHistoryGroup(before, after, kind);
  if (gestureChangeKind(kind) && group) history.recordDebounced(before, after, kind, group);
  else {
    history.flush();
    history.record(before, after, kind, group);
  }
}

function useGraphUpdates(
  ownership: { controlPort: ControlPort; graphId?: string },
  presentation: GraphPresentation,
  processors: readonly ExternalProcessorDescriptor[],
  graph: ReturnType<typeof useWorkspaceGraph>,
  savePositions: (model: AudioGraphModel) => void,
  catalogRevision?: number,
  failedProcessors?: FailedProcessorInstance[],
) {
  const { controlPort, graphId } = ownership;
  const { model, setModel } = graph;
  const owner = useMemo(() => ({ controlPort, graphId, active: true }), [controlPort, graphId]);
  const currentOwner = useRef(owner);
  currentOwner.current = owner;
  const currentModel = useRef(model);
  currentModel.current = model;
  const {
    clear: clearHistory,
    rebase: rebaseHistory,
    flush: flushHistory,
    record,
    recordDebounced,
    redo: nextRedoStep,
    undo: nextUndoStep,
  } = useUndoHistory<AudioGraphModel, AudioGraphChangeKind>();
  useEffect(() => {
    owner.active = true;
    clearHistory();
    return () => {
      owner.active = false;
    };
  }, [clearHistory, owner]);
  const parameters = useProcessorParameterPublisher(controlPort, graph.setPersistenceError);
  const { publish: publishProcessorParameter } = parameters;
  const applyModel = useGraphApplication(
    controlPort,
    presentation,
    processors,
    graph,
    { clear: clearHistory, rebase: rebaseHistory },
    currentModel,
    parameters,
    catalogRevision,
    failedProcessors,
  );
  const updateModel = useCallback(
    (nextModel: AudioGraphModel, kind: AudioGraphChangeKind) => {
      // Deferred commits from a replaced/unmounted canvas cannot revive its graph
      // or submit audio/layout edits through a different workspace owner.
      if (!liveWorkspaceOwner(currentOwner.current, owner)) return;
      // Canvas edits exist only while the owner has a loaded model/document.
      const previousModel = currentModel.current as AudioGraphModel;
      if (nextModel !== previousModel) {
        recordGraphEdit(
          { recordDebounced, record, flush: flushHistory },
          previousModel,
          nextModel,
          kind,
        );
      }
      currentModel.current = nextModel;
      setModel(nextModel);
      savePositions(nextModel);
      if (kind === 'processor_parameter' && previousModel && applyModel.canPublishParameters) {
        for (const update of changedProcessorParameters(previousModel, nextModel)) {
          publishProcessorParameter({
            ...update,
            expectedRevision: (graph.document as RuntimeGraphDocument).revision,
          });
        }
      } else if (kind !== 'layout') {
        applyModel.submit(nextModel, processorEditIntent(previousModel, nextModel, kind));
      }
    },
    [
      applyModel,
      graph.document?.revision,
      flushHistory,
      publishProcessorParameter,
      record,
      recordDebounced,
      setModel,
      savePositions,
      owner,
    ],
  );
  const { redo, undo } = useGraphHistoryNavigation({
    applyModel,
    currentModel,
    graphRevision: () => (graph.document as RuntimeGraphDocument).revision,
    nextRedoStep,
    nextUndoStep,
    publishProcessorParameter,
    savePositions,
    setModel,
  });
  return {
    clearHistory,
    redo,
    undo,
    updateModel,
    application: applyModel,
    parameters,
    parameterBusy: parameters.busy || parameters.failures.length > 0,
  };
}

function useSessionGraphRefresh(
  document: RuntimeGraphDocument | undefined,
  graph: ReturnType<typeof useWorkspaceGraph>,
  presentation: GraphPresentation,
  processors: readonly ExternalProcessorDescriptor[],
  clearHistory: () => void,
  application: GraphApplicationQueue,
  parameterBusy: boolean,
) {
  const applied = useRef<RuntimeGraphDocument | undefined>(undefined);
  const { setDocument, setModel, setErrorCode, setState } = graph;
  useEffect(() => {
    if (!document || applied.current === document) return;
    // A native settings refresh is not permission to discard an unapplied graph.
    if (
      parameterBusy ||
      graph.state === 'blocked' ||
      graph.state === 'applying' ||
      (graph.model && document.revision < graph.model.revision)
    )
      return;
    applied.current = document;
    application.reset(document);
    // Playback advances the native revision without replacing the authored graph.
    // Keep the canvas and undo history while adopting the new command baseline.
    if (
      graph.document &&
      JSON.stringify(graph.document.nodes) === JSON.stringify(document.nodes) &&
      JSON.stringify(graph.document.edges) === JSON.stringify(document.edges)
    ) {
      setDocument(document);
      setModel((previous) => previous && { ...previous, revision: document.revision });
      setErrorCode(null);
      setState('active');
      return;
    }
    const next = runtimeGraphToAudioGraph(document, presentation, processors);
    setDocument(document);
    setModel((previous) => ({
      ...next,
      nodes: next.nodes.map((node) => ({
        ...node,
        position: previous?.nodes.find((old) => old.id === node.id)?.position ?? node.position,
        size: previous?.nodes.find((old) => old.id === node.id)?.size,
      })),
    }));
    clearHistory();
    setErrorCode(null);
    setState('active');
  }, [
    document,
    graph.state,
    graph.document,
    graph.model,
    parameterBusy,
    presentation,
    processors,
    clearHistory,
    application,
    setDocument,
    setModel,
    setErrorCode,
    setState,
  ]);
}

export function RoutingWorkspace({
  failedProcessors,
  deviceRateAction,
  deviceDriverAction,
  graphId,
  interactionDisabled = false,
  sessionDocument,
  audioSession,
  controlPort,
  onBusyChange,
  ...viewOptions
}: RoutingWorkspaceProps) {
  const bufferSettings = audioSession?.virtualEndpointSettings;
  if (deviceDriverAction && audioSession)
    audioSession = { ...audioSession, virtualEndpointSettings: deviceDriverAction.settings };
  const [menuOpened, setMenuOpened] = useState(false);
  const { inventory, applications, audioDevices, processorCatalog, processors, refresh } =
    useWorkspaceInventory(controlPort, menuOpened);
  const presentation = useGraphPresentation();
  const graph = useWorkspaceGraph(controlPort, presentation, processors, sessionDocument, graphId);
  useUnavailableGraphRecovery(controlPort, presentation, processors, graph);
  const { errorCode, model, state } = graph;
  const layout = useGraphLayout(controlPort, graphId, model, graph.setModel);
  const { clearHistory, redo, undo, updateModel, application, parameterBusy, parameters } =
    useGraphUpdates(
      { controlPort, graphId },
      presentation,
      processors,
      graph,
      layout.updateNodes,
      viewOptions.catalogRevision,
      failedProcessors,
    );
  useWorkspaceBusy(onBusyChange, state, parameterBusy, layout);
  useSessionGraphRefresh(
    sessionDocument,
    graph,
    presentation,
    processors,
    clearHistory,
    application,
    parameterBusy,
  );
  useGraphHistoryShortcuts(undo, redo, layout.loaded && !layout.loading && !interactionDisabled);
  const nodeActions = useWorkspaceNodeActions({
    audioDevices,
    audioSession,
    applications,
    controlPort,
    deviceRateAction,
    deviceDriverAction,
    deviceBufferAction: viewOptions.deviceBufferAction,
    interactionDisabled,
    model,
    presentation,
    processors,
    refreshProcessors: refresh,
    state,
    updateModel,
  });

  const viewProps = {
    ...viewOptions,
    ...nodeActions,
    failedProcessors,
    documentRevision: graph.document?.revision ?? 0,
    graphId,
    interactionDisabled,
    deviceRateAction,
    deviceDriverAction,
    bufferSettings,
    processorCatalog,
    layout,
    application,
    audioSession,
    parameters,
    applications,
    audioDevices,
    controlPort,
    errorCode,
    inventory,
    onNodeMenuOpenedChange: setMenuOpened,
    persistenceError: graph.persistenceError,
    model,
    processors,
    state: state === 'active' && parameters.failures.length > 0 ? ('blocked' as const) : state,
    updateModel,
  };
  return (
    <>
      <WorkspaceView {...viewProps} />
      {nodeActions.virtualCreation.dialog}
    </>
  );
}

function useWorkspaceNodeActions(options: {
  audioDevices: readonly AudioDeviceDescriptor[];
  audioSession?: AudioSessionStatus;
  applications: readonly CapturableApplication[];
  controlPort: ControlPort;
  deviceRateAction?: RoutingWorkspaceProps['deviceRateAction'];
  deviceDriverAction?: RoutingWorkspaceProps['deviceDriverAction'];
  deviceBufferAction?: DeviceBufferAction;
  interactionDisabled: boolean;
  model: AudioGraphModel | null;
  presentation: GraphPresentation;
  processors: readonly ExternalProcessorDescriptor[];
  refreshProcessors: ReturnType<typeof useWorkspaceInventory>['refresh'];
  state: WorkspaceState;
  updateModel: UpdateModel;
}) {
  const virtualCreation = useVirtualEndpointCreation({
    owner: options.controlPort,
    model: options.model,
    settings: options.deviceDriverAction
      ? options.deviceDriverAction.settings
      : options.audioSession?.virtualEndpointSettings,
    createDevice: options.deviceDriverAction?.create,
    presentation: options.presentation,
    enabled:
      options.state === 'active' &&
      !options.interactionDisabled &&
      (options.deviceDriverAction
        ? options.deviceDriverAction.enabled
        : options.audioSession?.capabilities.virtualEndpoint === true),
    updateModel: (next) => options.updateModel(next, 'runtime'),
  });
  const addNode = useRoutingNodeCreation({
    tryVirtualAdd: virtualCreation.tryAdd,
    audioSession: options.audioSession,
    applications: options.applications,
    audioDevices: options.audioDevices,
    model: options.model,
    presentation: options.presentation,
    processors: options.processors,
    refreshProcessors: options.refreshProcessors,
    updateModel: options.updateModel,
  });
  const updateNodeGain = useRoutingNodeGain(options.model, options.updateModel);
  const updateNodeParameter = useRoutingNodeParameter(options.model, options.updateModel);
  const updateNodeParameters = useRoutingNodeParameters(options.model, options.updateModel);
  const updateChannelLayout = useProcessorLayoutChange(
    options.model,
    options.processors,
    options.presentation,
    options.updateModel,
  );
  const updateEndpoint = useVirtualEndpointUpdate(
    options.model,
    options.presentation,
    options.updateModel,
    options.audioSession?.virtualEndpointSettings,
    options.deviceRateAction,
    options.deviceDriverAction,
    options.deviceBufferAction,
  );
  return {
    addNode,
    updateChannelLayout,
    updateEndpoint,
    updateNodeGain,
    updateNodeParameter,
    updateNodeParameters,
    virtualCreation,
  };
}

function useWorkspaceBusy(
  onBusyChange: RoutingWorkspaceProps['onBusyChange'],
  state: WorkspaceState,
  parameterBusy: boolean,
  layout: ReturnType<typeof useGraphLayout>,
) {
  const busy = state !== 'active' || parameterBusy || layout.loading || layout.dirty;
  useEffect(() => onBusyChange?.(busy), [onBusyChange, busy]);
}

function useWorkspaceInventory(controlPort: ControlPort, menuOpened: boolean) {
  const inventory = useRoutingInventory(controlPort, menuOpened);
  const processorCatalog = useProcessorCatalog(controlPort);
  return {
    inventory,
    ...inventory,
    processorCatalog,
    processors: processorCatalog.processors,
    refresh: processorCatalog.refresh,
  };
}

interface WorkspaceViewProps {
  bufferSettings?: VirtualEndpointSettings;
  documentRevision: number;
  headerActions?: ReactNode;
  notice?: ReactNode;
  failedProcessors?: FailedProcessorInstance[];
  catalogRevision?: number;
  processorTimings?: RoutingWorkspaceProps['processorTimings'];
  graphId?: string;
  interactionDisabled: boolean;
  parameters: ReturnType<typeof useProcessorParameterPublisher>;
  deviceRateAction?: RoutingWorkspaceProps['deviceRateAction'];
  deviceDriverAction?: RoutingWorkspaceProps['deviceDriverAction'];
  deviceBufferAction?: DeviceBufferAction;
  processorCatalog: ReturnType<typeof useProcessorCatalog>;
  layout: ReturnType<typeof useGraphLayout>;
  persistenceError: string | null;
  inventory: ReturnType<typeof useRoutingInventory>;
  onNodeMenuOpenedChange: (opened: boolean) => void;
  application: GraphApplicationQueue;
  audioSession?: AudioSessionStatus;
  playbackPaused?: boolean;
  addNode: (kind: string, position: { x: number; y: number }) => void;
  applications: readonly CapturableApplication[];
  audioDevices: readonly AudioDeviceDescriptor[];
  controlPort: ControlPort;
  errorCode: string | null;
  model: AudioGraphModel | null;
  processors: readonly ExternalProcessorDescriptor[];
  onTitleChange?: (title: string) => void;
  state: WorkspaceState;
  title?: string;
  updateModel: (model: AudioGraphModel, kind: AudioGraphChangeKind) => void;
  updateEndpoint?: (nodeId: string, update: VirtualEndpointUpdate) => void;
  updateChannelLayout?: (nodeId: string, layoutId: string) => void;
  updateNodeGain?: (nodeId: string, gainDb: number) => void;
  updateNodeParameter?: (nodeId: string, parameterId: string, value: number) => void;
  updateNodeParameters?: (
    nodeId: string,
    updates: readonly { id: string; value: number }[],
  ) => void;
}

function useProcessorEditorLabels() {
  const { t } = useTranslation();
  return useMemo<ProcessorEditorLabels>(
    () => ({
      open: t('routing.processorEditor.open'),
      retry: t('routing.processorControl.retry'),
      failure: {
        display_unavailable: t('routing.processorEditor.failures.displayUnavailable'),
        host_window_failed: t('routing.processorEditor.failures.hostWindowFailed'),
        no_editor: t('routing.processorEditor.failures.noEditor'),
        plugin_rejected: t('routing.processorEditor.failures.pluginRejected'),
        worker_unavailable: t('routing.processorEditor.failures.workerUnavailable'),
      },
    }),
    [t],
  );
}

function usePresentedModel(props: WorkspaceViewProps) {
  const { t } = useTranslation();
  const {
    model,
    applications,
    audioDevices,
    inventory,
    audioSession,
    deviceRateAction,
    deviceDriverAction,
    deviceBufferAction,
  } = props;
  // A graph apply usually settles well under a second; only a lasting one is a warning.
  const state = useSustainedProblem(props.state, (current) => current !== 'active');
  const presented = useMemo(
    () =>
      model
        ? presentEndpointBuffers(
            presentVirtualNodeStatus(
              presentDriverEndpointControls(
                presentVirtualEndpointSettings(
                  presentApplicationReselection(
                    presentRoutingApplicationIcons(
                      presentHardwareNodeStatus(model, audioDevices, inventory.devicesObserved, t),
                      applications,
                    ),
                    t,
                  ),
                  audioSession?.virtualEndpointSettings,
                  t('audioSession.sampleRate'),
                  deviceRateAction,
                ),
                deviceDriverAction,
                t,
              ),
              state,
              audioSession,
              t,
            ),
            props.bufferSettings,
            deviceBufferAction,
            t,
          )
        : null,
    [
      applications,
      audioDevices,
      inventory.devicesObserved,
      model,
      audioSession,
      state,
      t,
      deviceRateAction,
      deviceDriverAction,
      deviceBufferAction,
      props.bufferSettings,
    ],
  );
  return useMemo(
    () =>
      presented &&
      presentNodeDisplayNames(
        presentProcessorLayouts(
          presentEqualizerSampleRate(
            presentProcessorTimings(presented, props.processorTimings, props.graphId),
            audioSession?.format.sampleRate,
          ) as AudioGraphModel,
          props.processors,
          t,
        ),
        props.layout.layout.nodeLabels,
      ),
    [
      presented,
      props.processors,
      t,
      props.processorTimings,
      props.graphId,
      audioSession?.format.sampleRate,
      props.layout.layout.nodeLabels,
    ],
  );
}

function driverEndpointControl(
  control: EndpointControl,
  device: VirtualEndpointSettings['devices'][number] | undefined,
  action: NonNullable<RoutingWorkspaceProps['deviceDriverAction']>,
  endpointError: string | null,
  t: TFunction,
): EndpointControl {
  const editable = device && action.enabled;
  return {
    ...control,
    displayName: device?.spec.displayName ?? control.displayName,
    renderChannels: device?.spec.render?.channels ?? control.renderChannels,
    captureChannels: device?.spec.capture?.channels ?? control.captureChannels,
    canRename: Boolean(editable && action.settings?.canRename),
    allowedRenderChannels: editable ? action.settings?.renderChannels : [],
    allowedCaptureChannels: editable ? action.settings?.captureChannels : [],
    observedRate: device
      ? { label: t('audioSession.sampleRate'), value: device.spec.defaultSampleRate }
      : undefined,
    sampleRateOptions: editable ? device.spec.supportedSampleRates : undefined,
    rateError: endpointError,
    channelsLabel: t('driverManagement.channels'),
    nativeFormats: device?.nativeFormats,
    nativeFormatLabels: {
      bitDepth: t('driverManagement.bitDepth'),
      mixed: t('driverManagement.mixed'),
      unknown: t('driverManagement.unknown'),
    },
    formatsDisabled: !action.enabled,
    formatError: endpointError,
  };
}

function presentDriverEndpointControls(
  model: AudioGraphModel,
  action: RoutingWorkspaceProps['deviceDriverAction'],
  t: TFunction,
): AudioGraphModel {
  if (!action) return model;
  return {
    ...model,
    nodes: model.nodes.map((node) => {
      const control = node.appearance.endpointControl;
      if (!control) return node;
      // Endpoint controls are constructed only after the native endpoint spec is validated.
      const id = ((node.runtime as NonNullable<AudioGraphNode['runtime']>).config as { id: string })
        .id;
      const device = action.settings?.devices.find((candidate) => candidate.spec.id === id);
      const endpointError =
        action.error && action.error.endpointId === id ? action.error.message : null;
      return {
        ...node,
        appearance: {
          ...node.appearance,
          title: device?.spec.displayName ?? node.appearance.title,
          endpointControl: driverEndpointControl(control, device, action, endpointError, t),
        },
      };
    }),
  };
}

function routingContextLabels(t: TFunction) {
  return {
    renameNode: t('routing.nodeName.title'),
    connectChannels: t('routing.context.connectChannels'),
    removeNode: t('routing.context.removeNode'),
    removeSelection: t('routing.context.removeSelection'),
    disconnect: t('routing.context.disconnect'),
    disconnectPort: t('routing.context.disconnectPort'),
    disconnectPorts: t('routing.context.disconnectPorts'),
    removeCable: t('routing.context.removeCable'),
    fitView: t('routing.context.fitView'),
    selectAll: t('routing.context.selectAll'),
    clearSelection: t('routing.context.clearSelection'),
  };
}

function WorkspaceView(props: WorkspaceViewProps) {
  const { t } = useTranslation();
  const titleRef = useRef<HTMLInputElement>(null);
  const recoveryFocus = useRemovedFocusReturn(titleRef, props.graphId);
  const layoutRecoveryFocus = useRemovedFocusReturn(titleRef, props.graphId);
  const {
    addNode,
    controlPort,
    errorCode,
    model,
    state,
    updateModel,
    updateEndpoint,
    updateNodeGain,
    updateNodeParameter,
    updateNodeParameters,
  } = props;
  const meterStore = useRoutingMeters(controlPort);
  const nodeControls = useWorkspaceNodeControls(props);
  const editorModel = useMemo(
    () => (model ? { ...model, revision: props.documentRevision } : null),
    [model, props.documentRevision],
  );
  const processorEditors = useProcessorEditors(controlPort, editorModel, props.graphId);
  const processorEditorLabels = useProcessorEditorLabels();
  const menuItems = useWorkspaceMenuItems(props, t);
  const presentedModel = usePresentedModel(props);
  const nodeRenderers = useNodeRenderers(props, t, nodeControls, presentedModel);
  const nodeNames = useNodeDisplayNameEditor(
    presentedModel,
    props.layout,
    props.interactionDisabled,
  );
  return (
    <section className={styles.root} data-state={state}>
      <WorkspaceToolbar
        titleRef={titleRef}
        title={props.title}
        onTitleChange={props.onTitleChange}
        headerActions={props.headerActions}
      />
      {props.notice}
      <GraphSyncError
        errorCode={errorCode}
        state={state}
        application={props.application}
        focusReturn={recoveryFocus}
      />
      <ProcessorParameterAlert
        parameters={props.parameters}
        applying={state === 'applying'}
        onReload={() => props.application.reload()}
      />
      <InventoryError inventory={props.inventory} />
      <ProcessorCatalogAlert catalog={props.processorCatalog} />
      <ProcessorEditorAlerts editors={processorEditors} />
      <GraphPersistenceWarning error={props.persistenceError} />
      <LayoutError layout={props.layout} focusReturn={layoutRecoveryFocus} />
      {presentedModel && props.layout.loaded ? (
        <div className={styles.canvas} inert={props.layout.loading}>
          <AudioGraphCanvas
            renderNodeControls={nodeRenderers.controls}
            renderNodeContent={nodeRenderers.content}
            viewport={props.layout.layout.viewport}
            onViewportChange={props.layout.updateViewport}
            contextMenuLabels={routingContextLabels(t)}
            addNodeLabel={t('routing.addNode')}
            ariaLabel={t('routing.canvasLabel')}
            model={presentedModel}
            meterStore={meterStore}
            nodeMenuItems={menuItems}
            onAddNode={addNode}
            onNodeMenuOpenedChange={props.onNodeMenuOpenedChange}
            onNodeRenameRequest={nodeNames.request}
            onModelChange={updateModel}
            onNodeEndpointChange={updateEndpoint}
            onNodeGainChange={updateNodeGain}
            onNodeParameterChange={updateNodeParameter}
            onNodeParametersChange={updateNodeParameters}
            onNodeChannelLayoutChange={props.updateChannelLayout}
            onProcessorEditorRetry={processorEditors.retry}
            processorEditorLabels={processorEditorLabels}
            processorEditorStatuses={processorEditors.statuses}
          />
        </div>
      ) : (
        <GraphPlaceholder state={state} layoutLoaded={props.layout.loaded} />
      )}
      {nodeNames.dialog}
    </section>
  );
}

function useWorkspaceMenuItems(props: WorkspaceViewProps, t: TFunction) {
  const paid = usePaidFeatures();
  return createNodeMenuItems({
    applications: props.applications,
    audioDevices: props.audioDevices,
    processors: props.processors,
    model: props.model,
    t,
    session: props.audioSession,
    processorEmptyLabel: t(processorCatalogPlaceholder(props.processorCatalog)),
    paid,
  });
}

function GraphPlaceholder({
  state,
  layoutLoaded,
}: {
  state: WorkspaceState;
  layoutLoaded: boolean;
}) {
  const { t } = useTranslation();
  const placeholderState = state === 'unavailable' || layoutLoaded ? state : 'loading';
  return (
    <div
      key={placeholderState}
      className={styles.placeholder}
      role={placeholderState === 'unavailable' ? 'alert' : 'status'}
    >
      {t(`routing.states.${placeholderState}`)}
    </div>
  );
}

function WorkspaceToolbar({
  titleRef,
  title,
  onTitleChange,
  headerActions,
}: Pick<WorkspaceViewProps, 'title' | 'onTitleChange' | 'headerActions'> & {
  titleRef: Ref<HTMLInputElement>;
}) {
  const { t } = useTranslation();
  return (
    <WorkspaceHeader>
      <div className={styles.titleBlock}>
        <TitleField
          ref={titleRef}
          aria-label={t('studio.graphName')}
          onBlur={(event) => {
            const normalized = event.currentTarget.value.trim();
            onTitleChange?.(normalized || t('studio.untitledGraph'));
          }}
          onChange={(event) => onTitleChange?.(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') event.currentTarget.blur();
          }}
          spellCheck={false}
          value={title ?? t('studio.untitledGraph')}
        />
      </div>
      {headerActions ? <div className={styles.headerActions}>{headerActions}</div> : null}
    </WorkspaceHeader>
  );
}

/** What node controls and recorder content read. Memoized so the canvas render functions
 * keep their identity and nodes re-render only when one of these changes. */
type NodeRenderInputs = Pick<
  WorkspaceViewProps,
  | 'application'
  | 'catalogRevision'
  | 'controlPort'
  | 'documentRevision'
  | 'failedProcessors'
  | 'graphId'
  | 'interactionDisabled'
  | 'playbackPaused'
  | 'state'
  | 'updateModel'
> & { idle: boolean; layoutDirty: boolean };

function useNodeRenderers(
  props: WorkspaceViewProps,
  t: TFunction,
  snapshot: import('../../core/control-port/types').GraphNodeControls | null,
  model: AudioGraphModel | null,
) {
  const idle = nodeControlIdle(props);
  const {
    application,
    catalogRevision,
    controlPort,
    documentRevision,
    failedProcessors,
    graphId,
    interactionDisabled,
    playbackPaused,
    state,
    updateModel,
  } = props;
  const layoutDirty = props.layout.dirty;
  const inputs = useMemo<NodeRenderInputs>(
    () => ({
      application,
      catalogRevision,
      controlPort,
      documentRevision,
      failedProcessors,
      graphId,
      idle,
      interactionDisabled,
      layoutDirty,
      playbackPaused,
      state,
      updateModel,
    }),
    [
      application,
      catalogRevision,
      controlPort,
      documentRevision,
      failedProcessors,
      graphId,
      idle,
      interactionDisabled,
      layoutDirty,
      playbackPaused,
      state,
      updateModel,
    ],
  );
  // Recorder edits apply to the committed model current when they are made, not the one
  // their node last rendered with; it is published on commit, never from a discarded render.
  const latestModel = useRef(model);
  useLayoutEffect(() => {
    latestModel.current = model;
  }, [model]);
  return {
    controls: useCallback(
      (node: AudioGraphNode) => nodeSignalControls(inputs, node, t, snapshot),
      [inputs, t, snapshot],
    ),
    content: useCallback(
      (node: AudioGraphNode) => recorderNodeContent(inputs, node, latestModel),
      [inputs],
    ),
  };
}

function recorderNodeContent(
  props: NodeRenderInputs,
  node: AudioGraphNode,
  model: { current: AudioGraphModel | null },
) {
  if (node.runtime?.type !== 'recorder' || !props.graphId) return null;
  return (
    <RecorderControls
      key={`${props.graphId}:${node.id}`}
      controlPort={props.controlPort}
      graphId={props.graphId}
      nodeId={node.id}
      name={audioGraphNodeTitle(node)}
      expectedRevision={props.documentRevision}
      poweredOn={node.runtime.control?.enabled !== false}
      playbackPaused={props.playbackPaused}
      enabled={
        props.state === 'active' &&
        !props.interactionDisabled &&
        node.runtime.control?.enabled !== false
      }
      channels={node.inputPorts.length}
      onChannelsChange={(channels) =>
        // Recorder content exists only while the workspace has a model.
        props.updateModel(
          updateRecorderChannels(model.current as AudioGraphModel, node.id, channels),
          'runtime',
        )
      }
    />
  );
}

function nodeControlIdle(props: WorkspaceViewProps) {
  return (
    props.state === 'active' &&
    !props.interactionDisabled &&
    !props.parameters.busy &&
    props.parameters.failures.length === 0 &&
    !props.layout.loading &&
    !props.layout.saving
  );
}

function useWorkspaceNodeControls(props: WorkspaceViewProps) {
  return useNodeControls(
    props.controlPort,
    props.graphId,
    props.documentRevision,
    nodeControlIdle(props),
    props.application,
  );
}

function nodeRecoveryControl(
  props: NodeRenderInputs,
  node: AudioGraphNode,
  enabled: boolean,
  disabled: boolean,
) {
  const failed =
    node.runtime?.type === 'provider'
      ? props.failedProcessors?.find(
          (candidate) => candidate.graphId === props.graphId && candidate.nodeId === node.id,
        )
      : undefined;
  const retryAvailable =
    failed !== undefined &&
    props.catalogRevision !== undefined &&
    props.controlPort.retryProcessor !== undefined;
  return retryAvailable ? (
    <ProcessorRecoveryControl
      busy={disabled || props.layoutDirty}
      catalogRevision={props.catalogRevision}
      enabled={enabled}
      failedProcessors={props.failedProcessors}
      graphId={props.graphId}
      isProcessor={node.runtime?.type === 'provider'}
      nodeId={node.id}
      nodeName={audioGraphNodeTitle(node)}
      onRetry={(target) => props.application.retryFailedProcessor(target)}
      supported
    />
  ) : null;
}

/** Built-ins expose only recovery for externally authored Off/Mute, alongside their bypass. */
function signalControlCapabilities(
  capabilities: { power: boolean; mute: boolean } | undefined,
  control: { enabled: boolean; muted: boolean },
  recoveryOnly: boolean,
) {
  return {
    power: capabilities?.power && (!recoveryOnly || !control.enabled),
    mute: capabilities?.mute && (!recoveryOnly || control.muted),
  };
}

function nodeSignalControls(
  props: NodeRenderInputs,
  node: AudioGraphNode,
  t: TFunction,
  snapshot: import('../../core/control-port/types').GraphNodeControls | null,
) {
  const control = node.runtime?.control ?? { enabled: true, muted: false };
  const recoveryOnly =
    node.runtime?.type === 'effect' ||
    node.runtime?.type === 'compressor' ||
    node.runtime?.type === 'limiter';
  if (recoveryOnly && control.enabled && !control.muted) return null;
  const nodeId = node.runtime?.binding?.nodeId ?? node.id;
  const entry = snapshot?.nodes.find((candidate) => candidate.nodeId === nodeId);
  const capabilities = signalControlCapabilities(entry?.capabilities, control, recoveryOnly);
  const disabled = !props.idle;
  const retry = nodeRecoveryControl(props, node, control.enabled, disabled);
  const retryAvailable = retry !== null;
  const graphId = props.graphId;
  if (!graphId || (!entry && !retryAvailable)) return null;
  if (!props.controlPort.updateNodeControl) return { power: retry };
  // Native disabled buttons own interaction gating; the graph queue validates each live command.
  const change = (value: { enabled?: boolean; muted?: boolean }) => {
    void props.application.setNodeControl(graphId, nodeId, value);
  };
  return {
    power: (
      <>
        {retry}
        {capabilities.power ? (
          <NodePowerButton
            enabled={control.enabled}
            disabled={disabled}
            label={t(
              control.enabled ? 'routing.nodeControl.powerOff' : 'routing.nodeControl.powerOn',
              { name: audioGraphNodeTitle(node) },
            )}
            onChange={() => change({ enabled: !control.enabled })}
          />
        ) : null}
      </>
    ),
    mute: capabilities.mute ? (
      <NodeMuteButton
        muted={control.muted}
        disabled={disabled}
        label={t(control.muted ? 'routing.nodeControl.unmute' : 'routing.nodeControl.mute', {
          name: audioGraphNodeTitle(node),
        })}
        onChange={() => change({ muted: !control.muted })}
      />
    ) : null,
  };
}

function GraphPersistenceWarning({ error }: { error: string | null }) {
  const { t } = useTranslation();
  return error ? (
    <InlineNotice layout="banner" tone="warning">
      {t('routing.persistenceWarning')}
    </InlineNotice>
  ) : null;
}

function LayoutError({
  layout,
  focusReturn,
}: {
  layout: ReturnType<typeof useGraphLayout>;
  focusReturn: ReturnType<typeof useRemovedFocusReturn>;
}) {
  const { t } = useTranslation();
  if (!layout.error) return null;
  return (
    <InlineNotice
      layout="banner"
      tone="warning"
      announcement="alert"
      inert={layout.loading || layout.saving}
      {...focusReturn}
    >
      {t('routing.layout.failed')}
      <ActionButton onClick={() => void layout.controller.retry()}>
        {t('routing.inventory.retry')}
      </ActionButton>
    </InlineNotice>
  );
}

function InventoryError({ inventory }: { inventory: ReturnType<typeof useRoutingInventory> }) {
  const { t } = useTranslation();
  if (!inventory.failed) return null;
  return (
    <RetryAlert
      framed
      message={t('routing.inventory.failed')}
      retryLabel={t('routing.inventory.retry')}
      busy={inventory.refreshing}
      onRetry={inventory.refresh}
    />
  );
}

function GraphSyncError({
  errorCode,
  state,
  application,
  focusReturn,
}: {
  errorCode: string | null;
  state: WorkspaceState;
  application: GraphApplicationQueue;
  focusReturn: ReturnType<typeof useRemovedFocusReturn>;
}) {
  const { t } = useTranslation();
  if (!errorCode) return null;
  return (
    <InlineNotice layout="banner" tone="critical" {...focusReturn}>
      <div>
        {errorCode === 'graph_apply_timeout'
          ? t('routing.recovery.timeout')
          : t(`routing.errors.${errorCode}`, {
              defaultValue: t('routing.errors.control_request_failed'),
            })}
      </div>
      {state === 'blocked' ? (
        <>
          <div>{t('routing.recovery.draftPreserved')}</div>
          {errorCode === 'graph_conflict_unresolved' ? (
            <RecoveryActions
              retryLabel={t('routing.recovery.retry')}
              reloadLabel={t('routing.recovery.reload')}
              onRetry={() => application.retry()}
              onReload={() => application.reload()}
            />
          ) : (
            <ActionButton onClick={() => application.retry()}>
              {t('routing.inventory.retry')}
            </ActionButton>
          )}
        </>
      ) : null}
    </InlineNotice>
  );
}
