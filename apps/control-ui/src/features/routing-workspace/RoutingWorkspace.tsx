import type { TFunction } from 'i18next';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
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
  RuntimeGraphDocument,
  VirtualEndpointRateError,
} from '../../core/control-port/types';
import { ControlPortError } from '../../core/control-port/types';
import { useUndoHistory } from '../../core/history/useUndoHistory';
import { RecoveryActions } from '../../core/ui/RecoveryActions';
import { RetryAlert } from '../../core/ui/RetryAlert';
import { WorkspaceHeader } from '../../core/ui/WorkspaceHeader';
import { RecorderControls } from '../recorder/RecorderControls';
import { applicationItemId } from './applicationIdentity';
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
import { NativeProcessorSurface } from './NativeProcessorSurface';
import { ProcessorCatalogAlert, ProcessorEditorAlerts } from './ProcessorControlAlerts';
import { ProcessorParameterAlert } from './ProcessorParameterAlert';
import {
  createProcessorMenuItems,
  processorCatalogPlaceholder,
  processorItemId,
} from './processorCatalogMenu';
import styles from './RoutingWorkspace.module.css';
import { useGraphLayout } from './useGraphLayout';
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
  const application = applications.find((candidate) => applicationItemId(candidate) === itemId);
  if (application) {
    return {
      bundleId: application.bundleId,
      kind: 'application',
      selector: application.selector,
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
  const processor = processors.find((candidate) => processorItemId(candidate) === itemId);
  if (processor) return { kind: 'provider', processor };
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

function workspaceStatusTone(state: WorkspaceState): 'critical' | 'positive' | 'warning' {
  if (state === 'active') return 'positive';
  return state === 'unavailable' ? 'critical' : 'warning';
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
) {
  return useCallback(
    (nodeId: string, update: VirtualEndpointUpdate) => {
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
    [model, presentation, updateModel, settings, rateAction],
  );
}

function useWorkspaceGraph(
  controlPort: ControlPort,
  presentation: GraphPresentation,
  processors: readonly ExternalProcessorDescriptor[],
  sessionDocument?: RuntimeGraphDocument,
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

function isNativeUndoTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable || target instanceof HTMLTextAreaElement) return true;
  if (!(target instanceof HTMLInputElement)) return false;
  return ['email', 'number', 'password', 'search', 'tel', 'text', 'url'].includes(target.type);
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
        (!event.metaKey && !event.ctrlKey) ||
        isNativeUndoTarget(event.target)
      ) {
        return;
      }
      if (runGraphHistoryShortcut(event, undo, redo)) event.preventDefault();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [redo, undo, enabled]);
}

function useGraphApplication(
  controlPort: ControlPort,
  presentation: GraphPresentation,
  processors: readonly ExternalProcessorDescriptor[],
  graph: ReturnType<typeof useWorkspaceGraph>,
  clearHistory: () => void,
  currentModel: { current: AudioGraphModel | null },
  parameters: Pick<
    ReturnType<typeof useProcessorParameterPublisher>,
    'whenIdle' | 'discardFailures'
  >,
) {
  const latest = useRef({ presentation, processors, graph, clearHistory });
  latest.current = { presentation, processors, graph, clearHistory };
  const queue = useMemo(
    () =>
      new GraphApplicationQueue(controlPort, {
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
        getDocument: () => latest.current.graph.document,
        getModel: () => currentModel.current,
        document: (document) => latest.current.graph.setDocument(document),
        error: (code) => latest.current.graph.setErrorCode(code),
        persistenceWarning: (code) => latest.current.graph.setPersistenceError(code),
        state: (state) => latest.current.graph.setState(state),
        restore: (document) => {
          const current = latest.current;
          const restored = runtimeGraphToAudioGraph(
            document,
            current.presentation,
            current.processors,
          );
          // Reload changes runtime state, not the geometry of surviving nodes.
          const model = {
            ...restored,
            nodes: restored.nodes.map((node) => ({
              ...node,
              position:
                currentModel.current?.nodes.find((old) => old.id === node.id)?.position ??
                node.position,
              size: currentModel.current?.nodes.find((old) => old.id === node.id)?.size,
            })),
          };
          currentModel.current = model;
          current.graph.setModel(model);
          current.clearHistory();
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

function useGraphUpdates(
  controlPort: ControlPort,
  presentation: GraphPresentation,
  processors: readonly ExternalProcessorDescriptor[],
  graph: ReturnType<typeof useWorkspaceGraph>,
  savePositions: (model: AudioGraphModel) => void,
) {
  const { model, setModel } = graph;
  const currentModel = useRef(model);
  currentModel.current = model;
  const {
    clear: clearHistory,
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
    clearHistory,
    currentModel,
    parameters,
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
          publishProcessorParameter(update);
        }
      } else if (kind !== 'layout') {
        applyModel.submit(nextModel, processorEditIntent(previousModel, nextModel, kind));
      }
    },
    [
      applyModel,
      flushHistory,
      publishProcessorParameter,
      record,
      recordDebounced,
      setModel,
      savePositions,
    ],
  );
  const restore = useCallback(
    (step: ReturnType<typeof nextUndoStep>) => {
      if (!step) return false;
      const restoredModel = restoreHistoryModel(step, currentModel.current);
      const previousModel = currentModel.current;
      currentModel.current = restoredModel;
      setModel(restoredModel);
      savePositions(restoredModel);
      if (step.kind === 'processor_parameter' && previousModel && applyModel.canPublishParameters) {
        for (const update of changedProcessorParameters(previousModel, restoredModel)) {
          publishProcessorParameter(update);
        }
      } else if (step.kind !== 'layout') {
        applyModel.submit(
          restoredModel,
          processorEditIntent(previousModel, restoredModel, step.kind),
        );
      }
      return true;
    },
    [applyModel, publishProcessorParameter, setModel, savePositions],
  );
  const undo = useCallback(() => restore(nextUndoStep()), [nextUndoStep, restore]);
  const redo = useCallback(() => restore(nextRedoStep()), [nextRedoStep, restore]);
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
  deviceRateAction,
  graphId,
  interactionDisabled = false,
  sessionDocument,
  audioSession,
  controlPort,
  onBusyChange,
  onTitleChange,
  title,
}: RoutingWorkspaceProps) {
  const [menuOpened, setMenuOpened] = useState(false);
  const { inventory, applications, audioDevices, processorCatalog, processors, refresh } =
    useWorkspaceInventory(controlPort, menuOpened);
  const presentation = useGraphPresentation();
  const graph = useWorkspaceGraph(controlPort, presentation, processors, sessionDocument);
  useUnavailableGraphRecovery(controlPort, presentation, processors, graph);
  const { errorCode, model, state } = graph;
  const layout = useGraphLayout(controlPort, graphId, model, graph.setModel);
  const { clearHistory, redo, undo, updateModel, application, parameterBusy, parameters } =
    useGraphUpdates(controlPort, presentation, processors, graph, layout.updateNodes);
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
  const virtualCreation = useVirtualEndpointCreation({
    owner: controlPort,
    model,
    settings: audioSession?.virtualEndpointSettings,
    presentation,
    enabled:
      state === 'active' &&
      !interactionDisabled &&
      audioSession?.capabilities.virtualEndpoint === true,
    updateModel: (next) => updateModel(next, 'runtime'),
  });
  const addNode = useRoutingNodeCreation({
    tryVirtualAdd: virtualCreation.tryAdd,
    audioSession,
    applications,
    audioDevices,
    model,
    presentation,
    processors,
    refreshProcessors: refresh,
    updateModel,
  });
  const updateNodeGain = useRoutingNodeGain(model, updateModel);
  const updateNodeParameter = useRoutingNodeParameter(model, updateModel);
  const updateEndpoint = useVirtualEndpointUpdate(
    model,
    presentation,
    updateModel,
    audioSession?.virtualEndpointSettings,
    deviceRateAction,
  );

  const viewProps = {
    graphId,
    interactionDisabled,
    deviceRateAction,
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
    onTitleChange,
    state: state === 'active' && parameters.failures.length > 0 ? ('blocked' as const) : state,
    title,
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
  graphId?: string;
  interactionDisabled: boolean;
  parameters: ReturnType<typeof useProcessorParameterPublisher>;
  deviceRateAction?: RoutingWorkspaceProps['deviceRateAction'];
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
  const { model, applications, audioDevices, inventory, audioSession, state, deviceRateAction } =
    props;
  return useMemo(
    () =>
      model
        ? presentVirtualNodeStatus(
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
    ],
  );
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
    title,
    updateModel,
    updateEndpoint,
    updateNodeGain,
    updateNodeParameter,
  } = props;
  const meterStore = useRoutingMeters(controlPort);
  const processorEditors = useProcessorEditors(controlPort, model, props.graphId);
  const processorEditorLabels = useProcessorEditorLabels();
  const graphTitle = title ?? t('studio.untitledGraph');
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
    <section className={styles.root}>
      <WorkspaceHeader className={styles.toolbar}>
        <div className={styles.titleBlock}>
          <input
            aria-label={t('studio.graphName')}
            className={styles.graphTitle}
            onBlur={(event) => {
              const normalized = event.currentTarget.value.trim();
              props.onTitleChange?.(normalized || t('studio.untitledGraph'));
            }}
            onChange={(event) => props.onTitleChange?.(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') event.currentTarget.blur();
            }}
            spellCheck={false}
            value={graphTitle}
          />
        </div>
        <span className={styles.syncState} data-tone={workspaceStatusTone(state)}>
          <span aria-hidden="true" />
          {t(`routing.states.${state}`)}
        </span>
      </WorkspaceHeader>
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
            renderNodeContent={(node) =>
              processorEditors.statuses.get(node.id)?.state === 'open' ? (
                <NativeProcessorSurface
                  port={controlPort}
                  nodeId={node.id}
                  title={node.appearance.title}
                />
              ) : (
                recorderNodeContent(props, node, presentedModel)
              )
            }
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
      enabled={props.state === 'active' && !props.interactionDisabled}
      channels={node.inputPorts.length}
      onChannelsChange={(channels) =>
        props.updateModel(updateRecorderChannels(model, node.id, channels), 'runtime')
      }
    />
  );
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
  return applications.length
    ? applications.map((application) => ({
        disabled: !supported,
        description: supported ? undefined : t('audioSession.captureRateLocked'),
        iconUrl: application.iconDataUrl ?? undefined,
        id: applicationItemId(application),
        label: application.displayName,
      }))
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
