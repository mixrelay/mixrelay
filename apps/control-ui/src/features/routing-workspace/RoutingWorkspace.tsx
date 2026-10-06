import type { TFunction } from 'i18next';
import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import {
  AudioGraphCanvas,
  type AudioGraphChangeKind,
  type AudioGraphNodeMenuItem,
  type ProcessorEditorLabels,
} from '../../core/audio-graph/AudioGraphCanvas';
import type { AudioGraphModel, AudioGraphNode } from '../../core/audio-graph/model';
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
import { NodeMuteButton, NodePowerButton } from '../../core/ui/NodeSignalControls';
import { hasPlatformModifier, isNativeTextEntryTarget } from '../../core/ui/platformShortcut';
import { RecoveryActions } from '../../core/ui/RecoveryActions';
import { RetryAlert } from '../../core/ui/RetryAlert';
import { WorkspaceHeader } from '../../core/ui/WorkspaceHeader';
import { RecorderControls } from '../recorder/RecorderControls';
import { applicationInventorySelection } from './applicationIdentity';
import { controlChanges, rebaseControls } from './controlReconciliation';
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
  updateVirtualEndpoint,
  type VirtualEndpointUpdate,
} from './model';
import { ProcessorCatalogAlert, ProcessorEditorAlerts } from './ProcessorControlAlerts';
import { ProcessorParameterAlert } from './ProcessorParameterAlert';
import { ProcessorRecoveryControl } from './ProcessorRecoveryControl';
import {
  createProcessorMenuItems,
  processorCatalogPlaceholder,
  processorSelectionForMenuItem,
} from './processorCatalogMenu';
import { presentProcessorTimings } from './processorTiming';
import styles from './RoutingWorkspace.module.css';
import { useGraphLayout } from './useGraphLayout';
import { useNodeControls } from './useNodeControls';
import { useProcessorCatalog } from './useProcessorCatalog';
import { useProcessorEditors } from './useProcessorEditors';
import { useProcessorParameterPublisher } from './useProcessorParameterPublisher';
import { useRoutingInventory } from './useRoutingInventory';
import { useRoutingMeters } from './useRoutingMeters';
import { useVirtualEndpointCreation } from './useVirtualEndpointCreation';
import { virtualDeviceMenuItem } from './virtualEndpointMenu';
import { presentApplicationReselection, presentVirtualNodeStatus } from './virtualNodeStatus';

type WorkspaceState = 'active' | 'applying' | 'loading' | 'unavailable' | 'blocked';
type UpdateModel = (model: AudioGraphModel, kind: AudioGraphChangeKind) => void;
const GRAPH_RECOVERY_MS = 500;
const ADD_PROCESSOR_DIRECTORY_ITEM_ID = 'add-processor-directory';
const RESCAN_PROCESSORS_ITEM_ID = 'rescan-processors';

export interface RoutingWorkspaceProps {
  deviceDriverAction?: {
    settings?: VirtualEndpointSettings;
    enabled: boolean;
    error: { endpointId: string | null; message: string } | null;
    change: (endpointId: string, update: VirtualEndpointUpdate) => void;
    create: (spec: VirtualEndpointSpec) => Promise<VirtualEndpointSpec | null>;
  };
  headerActions?: ReactNode;
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
  controlPort: ControlPort;
  onBusyChange?: (busy: boolean) => void;
  onTitleChange?: (title: string) => void;
  title?: string;
}

function useGraphPresentation(): GraphPresentation {
  const { t } = useTranslation();
  return useMemo<GraphPresentation>(
    () => ({
      application: t('routing.nodes.application'),
      attack: t('routing.nodes.attack'),
      audioInput: t('routing.nodes.audioInput'),
      audioOutput: t('routing.nodes.audioOutput'),
      recorder: t('recorder.title'),
      captureChannels: t('routing.nodes.captureChannels'),
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
      providerBusKinds: {
        main: t('routing.processorLayouts.main'),
        sidechain: t('routing.processorLayouts.sidechain'),
        auxiliary: t('routing.processorLayouts.auxiliary'),
      },
      ratio: t('routing.nodes.ratio'),
      renderChannels: t('routing.nodes.renderChannels'),
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
  if (itemId === 'recorder') return { kind: 'recorder' };
  if (itemId === 'limiter') return { kind: 'limiter' };
  const processor = processorSelectionForMenuItem(itemId, processors);
  if (processor) return { kind: 'provider', ...processor };
  return null;
}

function audioInputItemId(device: AudioDeviceDescriptor): string {
  return `audio-input:${device.uid}`;
}

function audioOutputItemId(device: AudioDeviceDescriptor): string {
  return `audio-output:${device.uid}`;
}

function supportedChannelCount(channels: number): number {
  return Math.max(1, Math.min(64, Math.trunc(channels)));
}

function useRoutingNodeGain(model: AudioGraphModel | null, updateModel: UpdateModel) {
  return useCallback(
    (nodeId: string, gainDb: number) => {
      if (model) updateModel(updateRoutingNodeGain(model, nodeId, gainDb), 'level');
    },
    [model, updateModel],
  );
}

function useRoutingNodeParameter(model: AudioGraphModel | null, updateModel: UpdateModel) {
  return useCallback(
    (nodeId: string, parameterId: string, value: number) => {
      if (model) {
        updateModel(
          updateRoutingNodeParameter(model, nodeId, parameterId, value),
          parameterId.startsWith('external:') ? 'processor_parameter' : 'level',
        );
      }
    },
    [model, updateModel],
  );
}

function useVirtualEndpointUpdate(
  model: AudioGraphModel | null,
  presentation: GraphPresentation,
  updateModel: UpdateModel,
  settings?: AudioSessionStatus['virtualEndpointSettings'],
  rateAction?: RoutingWorkspaceProps['deviceRateAction'],
  driverAction?: RoutingWorkspaceProps['deviceDriverAction'],
) {
  return useCallback(
    (nodeId: string, update: VirtualEndpointUpdate) => {
      if (driverAction) {
        const node = model?.nodes.find((candidate) => candidate.id === nodeId);
        const config = node?.runtime?.type === 'virtual_endpoint' ? node.runtime.config : null;
        const endpointId = config && typeof config === 'object' && 'id' in config ? config.id : null;
        if (driverAction.enabled && typeof endpointId === 'string') driverAction.change(endpointId, update);
        return;
      }
      if (update.sampleRate !== undefined) {
        const node = model?.nodes.find((candidate) => candidate.id === nodeId);
        const config = node?.runtime?.type === 'virtual_endpoint' ? node.runtime.config : null;
        const endpointId =
          config && typeof config === 'object' && 'id' in config ? config.id : null;
        if (rateAction?.enabled && typeof endpointId === 'string' && endpointId.trim())
          rateAction.change(endpointId, update.sampleRate);
        return;
      }
      if (model)
        updateModel(
          updateVirtualEndpoint(model, nodeId, update, presentation, settings),
          'runtime',
        );
    },
    [model, presentation, updateModel, settings, rateAction, driverAction],
  );
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
        nodes: refreshed.nodes.map((node) => ({
          ...node,
          position: current.nodes.find((old) => old.id === node.id)?.position ?? node.position,
          size: current.nodes.find((old) => old.id === node.id)?.size,
        })),
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
    let retryTimer: ReturnType<typeof setTimeout> | null = null;

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
      if (retryTimer) clearTimeout(retryTimer);
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
  failedProcessors?: FailedProcessorInstance[],
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
          latest.current.failedProcessors?.some(
            (failed) =>
              failed.graphId === graphId &&
              failed.nodeId === nodeId &&
              failed.instanceId === instanceId,
          ) ?? false,
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
        reconcile: (before, document) => {
          const changes = controlChanges(before, document);
          const model = currentModel.current;
          if (!changes || !model) return false;
          latest.current.history.rebase((entry) => rebaseControls(entry, changes));
          const next = { ...rebaseControls(model, changes), revision: document.revision };
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
                size: old ? old.size : savedLayout?.sizes?.[node.id],
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
  current: AudioGraphModel | null,
) {
  const { value, kind } = step;
  const currentRevision = current?.revision ?? value.revision;
  return {
    ...value,
    revision: kind === 'layout' ? currentRevision : Math.max(currentRevision, value.revision) + 1,
  };
}

type GraphHistoryStep = HistoryStep<AudioGraphModel, AudioGraphChangeKind>;

function useGraphHistoryNavigation(options: {
  applyModel: GraphApplicationQueue;
  currentModel: { current: AudioGraphModel | null };
  graphRevision?: number;
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
      const restoredModel = restoreHistoryModel(step, currentModel.current);
      const previousModel = currentModel.current;
      currentModel.current = restoredModel;
      setModel(restoredModel);
      savePositions(restoredModel);
      if (step.kind === 'processor_parameter' && previousModel && applyModel.canPublishParameters) {
        for (const update of changedProcessorParameters(previousModel, restoredModel)) {
          publishProcessorParameter({ ...update, expectedRevision: graphRevision ?? 0 });
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

function useGraphUpdates(
  controlPort: ControlPort,
  presentation: GraphPresentation,
  processors: readonly ExternalProcessorDescriptor[],
  graph: ReturnType<typeof useWorkspaceGraph>,
  savePositions: (model: AudioGraphModel) => void,
  catalogRevision?: number,
  failedProcessors?: FailedProcessorInstance[],
) {
  const { model, setModel } = graph;
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
    void controlPort;
    clearHistory();
  }, [clearHistory, controlPort]);
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
      const previousModel = currentModel.current;
      if (previousModel && nextModel !== previousModel) {
        const group = graphHistoryGroup(previousModel, nextModel, kind);
        if ((kind === 'level' || kind === 'processor_parameter') && group) {
          recordDebounced(previousModel, nextModel, kind, group);
        } else {
          flushHistory();
          record(previousModel, nextModel, kind, group);
        }
      }
      currentModel.current = nextModel;
      setModel(nextModel);
      savePositions(nextModel);
      if (kind === 'processor_parameter' && previousModel && applyModel.canPublishParameters) {
        for (const update of changedProcessorParameters(previousModel, nextModel)) {
          publishProcessorParameter({ ...update, expectedRevision: graph.document?.revision ?? 0 });
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
    ],
  );
  const { redo, undo } = useGraphHistoryNavigation({
    applyModel,
    currentModel,
    graphRevision: graph.document?.revision,
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
  catalogRevision,
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
      controlPort,
      presentation,
      processors,
      graph,
      layout.updateNodes,
      catalogRevision,
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
  const { addNode, updateEndpoint, updateNodeGain, updateNodeParameter, virtualCreation } =
    useWorkspaceNodeActions({
      audioDevices,
      audioSession,
      applications,
      controlPort,
      deviceRateAction,
      deviceDriverAction,
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
    failedProcessors,
    catalogRevision,
    documentRevision: graph.document?.revision ?? 0,
    graphId,
    interactionDisabled,
    deviceRateAction,
    deviceDriverAction,
    processorCatalog,
    layout,
    application,
    audioSession,
    parameters,
    addNode,
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
    updateEndpoint,
    updateNodeGain,
    updateNodeParameter,
  };
  return (
    <>
      <WorkspaceView {...viewProps} />
      {virtualCreation.dialog}
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
    settings: options.deviceDriverAction ? options.deviceDriverAction.settings : options.audioSession?.virtualEndpointSettings,
    createDevice: options.deviceDriverAction?.create,
    presentation: options.presentation,
    enabled:
      options.state === 'active' &&
      !options.interactionDisabled &&
      (options.deviceDriverAction ? options.deviceDriverAction.enabled : options.audioSession?.capabilities.virtualEndpoint === true),
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
  const updateEndpoint = useVirtualEndpointUpdate(
    options.model,
    options.presentation,
    options.updateModel,
    options.audioSession?.virtualEndpointSettings,
    options.deviceRateAction,
    options.deviceDriverAction,
  );
  return { addNode, updateEndpoint, updateNodeGain, updateNodeParameter, virtualCreation };
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
  documentRevision: number;
  headerActions?: ReactNode;
  failedProcessors?: FailedProcessorInstance[];
  catalogRevision?: number;
  processorTimings?: RoutingWorkspaceProps['processorTimings'];
  graphId?: string;
  interactionDisabled: boolean;
  parameters: ReturnType<typeof useProcessorParameterPublisher>;
  deviceRateAction?: RoutingWorkspaceProps['deviceRateAction'];
  deviceDriverAction?: RoutingWorkspaceProps['deviceDriverAction'];
  processorCatalog: ReturnType<typeof useProcessorCatalog>;
  layout: ReturnType<typeof useGraphLayout>;
  persistenceError: string | null;
  inventory: ReturnType<typeof useRoutingInventory>;
  onNodeMenuOpenedChange: (opened: boolean) => void;
  application: GraphApplicationQueue;
  audioSession?: AudioSessionStatus;
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
  updateEndpoint: (nodeId: string, update: VirtualEndpointUpdate) => void;
  updateNodeGain: (nodeId: string, gainDb: number) => void;
  updateNodeParameter: (nodeId: string, parameterId: string, value: number) => void;
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
  const { model, applications, audioDevices, inventory, audioSession, state, deviceRateAction, deviceDriverAction } =
    props;
  const presented = useMemo(
    () =>
      model
        ? presentVirtualNodeStatus(
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
    ],
  );
  return useMemo(
    () => presentProcessorTimings(presented, props.processorTimings, props.graphId),
    [presented, props.processorTimings, props.graphId],
  );
}

function presentDriverEndpointControls(
  model: AudioGraphModel,
  action: RoutingWorkspaceProps['deviceDriverAction'],
  t: TFunction,
): AudioGraphModel {
  if (!action) return model;
  return { ...model, nodes: model.nodes.map((node) => {
    const control = node.appearance.endpointControl;
    if (!control) return node;
    const config = node.runtime?.config;
    const id = config && typeof config === 'object' && 'id' in config ? config.id : null;
    const device = action.settings?.devices.find((candidate) => candidate.spec.id === id);
    const endpointError = action.error && action.error.endpointId === id ? action.error.message : null;
    return { ...node, appearance: { ...node.appearance,
      title: device?.spec.displayName ?? node.appearance.title,
      endpointControl: { ...control,
        displayName: device?.spec.displayName ?? control.displayName,
        renderChannels: device?.spec.render?.channels ?? control.renderChannels,
        captureChannels: device?.spec.capture?.channels ?? control.captureChannels,
        canRename: Boolean(device && action.enabled && action.settings?.canRename),
        allowedRenderChannels: device && action.enabled ? action.settings?.renderChannels : [],
        allowedCaptureChannels: device && action.enabled ? action.settings?.captureChannels : [],
        observedRate: device ? { label: t('audioSession.sampleRate'), value: device.spec.defaultSampleRate } : undefined,
        sampleRateOptions: device && action.enabled ? device.spec.supportedSampleRates : undefined,
        rateError: endpointError,
        nativeFormats: device?.nativeFormats,
        nativeFormatLabels: { render: t('driverManagement.renderFormat'), capture: t('driverManagement.captureFormat'), unknown: t('driverManagement.unknown') },
        formatsDisabled: !action.enabled,
        formatError: endpointError,
      },
    } };
  }) };
}

function routingContextLabels(t: TFunction) {
  return {
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
  const {
    addNode,
    applications,
    audioDevices,
    controlPort,
    errorCode,
    model,
    processors,
    state,
    updateModel,
    updateEndpoint,
    updateNodeGain,
    updateNodeParameter,
  } = props;
  const meterStore = useRoutingMeters(controlPort);
  const nodeControls = useWorkspaceNodeControls(props);
  const editorModel = useMemo(
    () => (model ? { ...model, revision: props.documentRevision } : null),
    [model, props.documentRevision],
  );
  const processorEditors = useProcessorEditors(controlPort, editorModel, props.graphId);
  const processorEditorLabels = useProcessorEditorLabels();
  const menuItems = createNodeMenuItems(
    applications,
    audioDevices,
    processors,
    model,
    t,
    props.audioSession,
    t(processorCatalogPlaceholder(props.processorCatalog)),
  );
  const presentedModel = usePresentedModel(props);
  return (
    <section className={styles.root} data-state={state}>
      <WorkspaceToolbar
        title={props.title}
        onTitleChange={props.onTitleChange}
        headerActions={props.headerActions}
      />
      <GraphSyncError errorCode={errorCode} state={state} application={props.application} />
      <ProcessorParameterAlert
        parameters={props.parameters}
        applying={state === 'applying'}
        onReload={() => props.application.reload()}
      />
      <InventoryError inventory={props.inventory} />
      <ProcessorCatalogAlert catalog={props.processorCatalog} />
      <ProcessorEditorAlerts editors={processorEditors} />
      <GraphPersistenceWarning error={props.persistenceError} />
      <LayoutError layout={props.layout} />
      {presentedModel && props.layout.loaded ? (
        <div className={styles.canvas} inert={props.layout.loading}>
          <AudioGraphCanvas
            renderNodeControls={(node) => nodeSignalControls(props, node, t, nodeControls)}
            renderNodeContent={(node) => recorderNodeContent(props, node, presentedModel)}
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
            onModelChange={updateModel}
            onNodeEndpointChange={updateEndpoint}
            onNodeGainChange={updateNodeGain}
            onNodeParameterChange={updateNodeParameter}
            onProcessorEditorRetry={processorEditors.retry}
            processorEditorLabels={processorEditorLabels}
            processorEditorStatuses={processorEditors.statuses}
          />
        </div>
      ) : (
        <div className={styles.placeholder}>
          {t(`routing.states.${props.layout.loaded ? state : 'loading'}`)}
        </div>
      )}
    </section>
  );
}

function WorkspaceToolbar({
  title,
  onTitleChange,
  headerActions,
}: Pick<WorkspaceViewProps, 'title' | 'onTitleChange' | 'headerActions'>) {
  const { t } = useTranslation();
  return (
    <WorkspaceHeader className={styles.toolbar}>
      <div className={styles.titleBlock}>
        <input
          aria-label={t('studio.graphName')}
          className={styles.graphTitle}
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

function recorderNodeContent(
  props: WorkspaceViewProps,
  node: AudioGraphNode,
  model: AudioGraphModel,
) {
  if (node.runtime?.type !== 'recorder' || !props.graphId) return null;
  return (
    <RecorderControls
      key={`${props.graphId}:${node.id}`}
      controlPort={props.controlPort}
      graphId={props.graphId}
      nodeId={node.id}
      expectedRevision={props.documentRevision}
      poweredOn={node.runtime.control?.enabled !== false}
      enabled={
        props.state === 'active' &&
        !props.interactionDisabled &&
        node.runtime.control?.enabled !== false
      }
      channels={node.inputPorts.length}
      onChannelsChange={(channels) =>
        props.updateModel(updateRecorderChannels(model, node.id, channels), 'runtime')
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

function nodeSignalControls(
  props: WorkspaceViewProps,
  node: AudioGraphNode,
  t: TFunction,
  snapshot: import('../../core/control-port/types').GraphNodeControls | null,
) {
  const nodeId = node.runtime?.binding?.nodeId ?? node.id;
  const entry = snapshot?.nodes.find((candidate) => candidate.nodeId === nodeId);
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
  const control = node.runtime?.control ?? { enabled: true, muted: false };
  const disabled = !nodeControlIdle(props);
  const retry = retryAvailable ? (
    <ProcessorRecoveryControl
      busy={disabled || props.layout.dirty}
      catalogRevision={props.catalogRevision}
      enabled={control.enabled}
      failedProcessors={props.failedProcessors}
      graphId={props.graphId}
      isProcessor={node.runtime?.type === 'provider'}
      nodeId={node.id}
      nodeName={node.appearance.title}
      onRetry={(target) => props.application.retryFailedProcessor(target)}
      supported
    />
  ) : null;
  if (!props.graphId || (!entry && !retryAvailable)) return null;
  const change = (value: { enabled?: boolean; muted?: boolean }) => {
    if (!disabled && props.graphId && props.controlPort.updateNodeControl)
      void props.application.setNodeControl(props.graphId, nodeId, value);
  };
  return {
    power: (
      <>
        {retry}
        {entry?.capabilities.power ? (
          <NodePowerButton
            enabled={control.enabled}
            disabled={disabled}
            label={t(
              control.enabled ? 'routing.nodeControl.powerOff' : 'routing.nodeControl.powerOn',
              { name: node.appearance.title },
            )}
            onChange={() => change({ enabled: !control.enabled })}
          />
        ) : null}
      </>
    ),
    mute: entry?.capabilities.mute ? (
      <NodeMuteButton
        muted={control.muted}
        disabled={disabled}
        label={t(control.muted ? 'routing.nodeControl.unmute' : 'routing.nodeControl.mute', {
          name: node.appearance.title,
        })}
        onChange={() => change({ muted: !control.muted })}
      />
    ) : null,
  };
}

function GraphPersistenceWarning({ error }: { error: string | null }) {
  const { t } = useTranslation();
  return error ? (
    <div className={styles.warning} role="status">
      {t('routing.persistenceWarning')}
    </div>
  ) : null;
}

function LayoutError({ layout }: { layout: ReturnType<typeof useGraphLayout> }) {
  const { t } = useTranslation();
  if (!layout.error) return null;
  return (
    <div className={styles.warning} role="alert" inert={layout.loading || layout.saving}>
      {t('routing.layout.failed')}
      <RecoveryActions
        retryLabel={t('routing.layout.retry')}
        reloadLabel={t('routing.layout.reload')}
        onRetry={() => void layout.controller.retry()}
        onReload={() => void layout.controller.reload()}
      />
    </div>
  );
}

function InventoryError({ inventory }: { inventory: ReturnType<typeof useRoutingInventory> }) {
  const { t } = useTranslation();
  if (!inventory.failed) return null;
  return (
    <RetryAlert
      className={styles.error}
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
}: {
  errorCode: string | null;
  state: WorkspaceState;
  application: GraphApplicationQueue;
}) {
  const { t } = useTranslation();
  if (!errorCode) return null;
  return (
    <div className={styles.error} role="alert">
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
          <RecoveryActions
            retryLabel={t('routing.recovery.retry')}
            reloadLabel={t('routing.recovery.reload')}
            onRetry={() => application.retry()}
            onReload={() => application.reload()}
          />
        </>
      ) : null}
    </div>
  );
}

function createApplicationMenuItems(
  applications: readonly CapturableApplication[],
  t: TFunction,
  supported = true,
): AudioGraphNodeMenuItem[] {
  const items = new Map<string, AudioGraphNodeMenuItem>();
  for (const application of applications) {
    const selection = applicationInventorySelection(application);
    if (!selection) continue;
    const iconUrl = application.iconDataUrl || undefined;
    const previous = items.get(selection.id);
    if (previous) {
      if (!previous.iconUrl && iconUrl) previous.iconUrl = iconUrl;
      continue;
    }
    items.set(selection.id, {
      disabled: !supported,
      description: supported ? undefined : t('audioSession.captureRateLocked'),
      iconUrl,
      id: selection.id,
      label: application.displayName,
    });
  }
  return items.size
    ? [...items.values()]
    : [{ disabled: true, id: 'no-applications', label: t('routing.noApplications') }];
}

function createNodeMenuItems(
  applications: readonly CapturableApplication[],
  audioDevices: readonly AudioDeviceDescriptor[],
  processors: readonly ExternalProcessorDescriptor[],
  model: AudioGraphModel | null,
  t: TFunction,
  session?: AudioSessionStatus,
  processorEmptyLabel = t('routing.noProcessors'),
): AudioGraphNodeMenuItem[] {
  const virtualIds = new Set(
    session?.virtualEndpointSettings?.devices.flatMap((device) => device.nativeDeviceIds),
  );
  const hostDevices = audioDevices.filter((device) => !virtualIds.has(device.uid));
  const inputs = hostDevices.filter((device) => device.inputChannels > 0);
  const outputs = hostDevices.filter((device) => device.outputChannels > 0);
  const applicationItems = createApplicationMenuItems(
    applications,
    t,
    session?.capabilities.applicationCapture,
  );
  const inputItems: AudioGraphNodeMenuItem[] = inputs.length
    ? inputs.map((device) => ({
        disabled: session?.capabilities.hardwareInput === false,
        description:
          session?.capabilities.hardwareInput === false
            ? t('audioSession.unsupported')
            : `${device.inputChannels} CH`,
        id: audioInputItemId(device),
        label: device.displayName,
      }))
    : [{ disabled: true, id: 'no-input-devices', label: t('routing.noInputDevices') }];
  const outputItems: AudioGraphNodeMenuItem[] = outputs.length
    ? outputs.map((device) => ({
        disabled: session?.capabilities.hardwareOutput === false,
        description:
          session?.capabilities.hardwareOutput === false
            ? t('audioSession.unsupported')
            : `${device.outputChannels} CH`,
        id: audioOutputItemId(device),
        label: device.displayName,
      }))
    : [{ disabled: true, id: 'no-output-devices', label: t('routing.noOutputDevices') }];
  const processorItems = createProcessorMenuItems(processors, t, processorEmptyLabel);
  return [
    {
      children: [
        virtualDeviceMenuItem('input', model, t, session),
        { id: 'virtual-input-divider', kind: 'divider', label: '' },
        {
          id: 'running-applications-label',
          kind: 'label',
          label: t('routing.groups.runningApplications'),
        },
        ...applicationItems,
        { id: 'source-divider', kind: 'divider', label: '' },
        { id: 'audio-inputs-label', kind: 'label', label: t('routing.groups.audioInputs') },
        ...inputItems,
      ],
      id: 'source',
      label: t('routing.groups.source'),
    },
    {
      children: [
        {
          description: t('routing.nodes.compressorDescription'),
          id: 'compressor',
          label: t('routing.nodes.compressor'),
        },
        {
          description: t('routing.nodes.limiterDescription'),
          id: 'limiter',
          label: t('routing.nodes.limiter'),
        },
        { id: 'processor-divider', kind: 'divider', label: '' },
        ...processorItems,
        { id: 'processor-actions-divider', kind: 'divider', label: '' },
        {
          description: t('routing.addProcessorDirectoryDescription'),
          id: ADD_PROCESSOR_DIRECTORY_ITEM_ID,
          label: t('routing.addProcessorDirectory'),
        },
        { id: RESCAN_PROCESSORS_ITEM_ID, label: t('routing.rescanProcessors') },
      ],
      id: 'processor',
      label: t('routing.groups.processor'),
    },
    {
      children: [
        virtualDeviceMenuItem('output', model, t, session),
        { id: 'recorder', label: t('recorder.title') },
        { id: 'virtual-output-divider', kind: 'divider', label: '' },
        { id: 'audio-outputs-label', kind: 'label', label: t('routing.groups.audioOutputs') },
        ...outputItems,
      ],
      id: 'output',
      label: t('routing.groups.output'),
    },
  ];
}
