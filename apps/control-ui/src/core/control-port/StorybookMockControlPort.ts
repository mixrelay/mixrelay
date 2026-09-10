import type {
  AudioDeviceDescriptor,
  CapturableApplication,
  ControlPort,
  ExternalProcessorCatalog,
  GraphApplyRequest,
  GraphApplyResponse,
  GraphCatalog,
  GraphCatalogRequest,
  GraphLayoutRequest,
  GraphLayoutResponse,
  GraphRuntimeStatus,
  MeterSnapshot,
  MixRelayStatus,
  ProcessorEditorFrame,
  ProcessorEditorInput,
  ProcessorEditorStatus,
  ProcessorParameterUpdateResponse,
  RecorderRequest,
  RecorderStatus,
  RuntimeGraphDocument,
} from './types';

export interface StorybookControlState {
  applications?: CapturableApplication[];
  audioDevices?: AudioDeviceDescriptor[];
  graph: RuntimeGraphDocument;
  graphStatus: GraphRuntimeStatus;
  meters?: MeterSnapshot;
  processors?: ExternalProcessorCatalog;
  status: MixRelayStatus;
}

export class StorybookMockControlPort implements ControlPort {
  public async getNodeControls(graphId: string): Promise<import('./types').GraphNodeControls> {
    const document =
      graphId === this.activeGraphId ? this.state.graph : this.savedGraphs.get(graphId)?.document;
    if (!document) throw new Error('graph_invalid');
    return {
      graphId,
      revision: document.revision,
      nodes: document.nodes.map((node) => ({
        nodeId: node.id,
        label: node.label,
        kind: node.type,
        control: node.control ?? { enabled: true, muted: false },
        capabilities: { power: true, mute: true, recorder: node.type === 'recorder' },
      })),
    };
  }
  public async updateNodeControl(
    graphId: string,
    nodeId: string,
    request: import('./types').NodeControlRequest,
  ): Promise<GraphApplyResponse> {
    if (
      graphId !== this.activeGraphId ||
      request.expectedRevision !== this.state.graph.revision ||
      !this.state.graph.nodes.some((node) => node.id === nodeId)
    )
      throw new Error('graph_revision_stale');
    return this.applyGraph({
      ...this.state.graph,
      revision: request.expectedRevision + 1,
      nodes: this.state.graph.nodes.map((node) =>
        node.id === nodeId
          ? {
              ...node,
              control: {
                enabled: request.enabled ?? node.control?.enabled ?? true,
                muted: request.muted ?? node.control?.muted ?? false,
              },
            }
          : node,
      ),
    });
  }
  public async recorderStatus(_graphId: string, _nodeId: string): Promise<RecorderStatus> {
    return {
      state: 'stopped',
      path: null,
      sampleRate: null,
      encoding: null,
      frames: 0,
      error: null,
    };
  }

  public async controlRecorder(
    _graphId: string,
    _nodeId: string,
    _request: RecorderRequest,
  ): Promise<RecorderStatus> {
    // The preview cannot write WAV files; never present simulated recording as a saved take.
    throw new Error('Recording requires the local audio engine.');
  }

  private readonly layouts = new Map<string, GraphLayoutResponse>();

  public async getGraphLayout(graphId: string): Promise<GraphLayoutResponse> {
    if (!this.savedGraphs.has(graphId)) throw new Error('graph_invalid');
    return structuredClone(
      this.layouts.get(graphId) ?? {
        graphId,
        layoutRevision: 0,
        layout: { positions: {} },
      },
    );
  }

  public async setGraphLayout(
    graphId: string,
    request: GraphLayoutRequest,
  ): Promise<GraphLayoutResponse> {
    const saved = await this.getGraphLayout(graphId);
    if (request.expectedLayoutRevision !== saved.layoutRevision)
      throw new Error('graph_revision_stale');
    const next = {
      graphId,
      layoutRevision: saved.layoutRevision + 1,
      layout: structuredClone(request.layout),
    };
    this.layouts.set(graphId, next);
    return structuredClone(next);
  }
  private readonly editorStates = new Map<string, ProcessorEditorStatus>();
  private catalogRevision = 1;
  private activeGraphId = 'graph-1';
  private readonly savedGraphs = new Map<
    string,
    { name: string; document: RuntimeGraphDocument }
  >();

  public constructor(private readonly state: StorybookControlState) {
    this.savedGraphs.set(this.activeGraphId, {
      name: 'Untitled Graph',
      document: structuredClone(state.graph),
    });
  }

  public async getGraphCatalog(): Promise<GraphCatalog> {
    return structuredClone({
      protocolVersion: 1,
      catalogRevision: this.catalogRevision,
      activeId: this.activeGraphId,
      graphs: Array.from(this.savedGraphs, ([id, value]) => ({ id, name: value.name })),
      document: this.state.graph,
    });
  }

  public async mutateGraphCatalog(request: GraphCatalogRequest): Promise<GraphCatalog> {
    if (
      request.expectedRevision !== this.state.graph.revision ||
      request.expectedCatalogRevision !== this.catalogRevision
    ) {
      throw new Error('graph_revision_stale');
    }
    if (request.action === 'rename') {
      const graph = this.savedGraphs.get(request.id);
      if (!graph) throw new Error('graph_invalid');
      graph.name = request.name;
    } else {
      const current = this.savedGraphs.get(this.activeGraphId);
      if (!current) throw new Error('graph_invalid');
      current.document = structuredClone(this.state.graph);
      const id = request.action === 'create' ? `graph-${this.catalogRevision + 1}` : request.id;
      if (request.action === 'create') {
        this.savedGraphs.set(id, {
          name: request.name,
          document: { schemaVersion: 2, revision: 0, nodes: [], edges: [] },
        });
      }
      const target = this.savedGraphs.get(id);
      if (!target) throw new Error('graph_invalid');
      this.activeGraphId = id;
      await this.applyGraph({
        ...structuredClone(target.document),
        revision: this.state.graph.revision + 1,
      });
    }
    this.catalogRevision += 1;
    return this.getGraphCatalog();
  }

  public async applyGraph(request: GraphApplyRequest): Promise<GraphApplyResponse> {
    const document = preserveMockProcessorState(this.state.graph, request);
    this.state.graph = document;
    const current = this.savedGraphs.get(this.activeGraphId);
    if (!current) throw new Error('graph_invalid');
    current.document = structuredClone(document);
    this.state.graphStatus = {
      activeRevision: document.revision,
      protocolVersion: 1,
      submittedRevision: document.revision,
    };
    return { protocolVersion: 1, revision: document.revision, state: 'active' };
  }

  public async getGraph(): Promise<RuntimeGraphDocument> {
    return this.state.graph;
  }

  public async getGraphStatus(): Promise<GraphRuntimeStatus> {
    return this.state.graphStatus;
  }

  public async getMeters(): Promise<MeterSnapshot> {
    return (
      this.state.meters ?? {
        nodes: [],
        protocolVersion: 1,
        revision: this.state.graphStatus.activeRevision,
      }
    );
  }

  public async getProcessors(): Promise<ExternalProcessorCatalog> {
    return this.state.processors ?? { processors: [], protocolVersion: 1 };
  }

  public async getProcessorEditor(nodeId: string): Promise<ProcessorEditorStatus> {
    return this.editorStates.get(nodeId) ?? closedEditor(nodeId);
  }

  public async getProcessorEditorFrame(_nodeId: string): Promise<ProcessorEditorFrame> {
    throw new Error('Native editor images are unavailable in Storybook');
  }

  public async sendProcessorEditorInput(
    _nodeId: string,
    _input: ProcessorEditorInput,
  ): Promise<void> {}

  public async rescanProcessors(_paths: string[]): Promise<ExternalProcessorCatalog> {
    return this.getProcessors();
  }

  public async getStatus(): Promise<MixRelayStatus> {
    return this.state.status;
  }

  public async setProcessorParameter(
    nodeId: string,
    parameterId: number,
    value: number,
  ): Promise<ProcessorParameterUpdateResponse> {
    const node = this.state.graph.nodes.find((candidate) => candidate.id === nodeId);
    if (
      node?.type !== 'provider' ||
      !Number.isInteger(parameterId) ||
      parameterId < 0 ||
      !Number.isFinite(value) ||
      value < 0 ||
      value > 1
    )
      throw new Error('graph_invalid');
    const config = record(node.config);
    const parameters = record(config.parameters);
    node.config = {
      ...config,
      parameters: {
        ...parameters,
        parameterValues: { ...record(parameters.parameterValues), [parameterId]: value },
      },
    };
    const current = this.savedGraphs.get(this.activeGraphId);
    if (current) current.document = structuredClone(this.state.graph);
    return { nodeId, parameterId, protocolVersion: 1, value };
  }

  public async setProcessorEditor(nodeId: string, open: boolean): Promise<ProcessorEditorStatus> {
    const status = {
      ...closedEditor(nodeId),
      state: open ? ('open' as const) : ('closed' as const),
    };
    this.editorStates.set(nodeId, status);
    return status;
  }

  public async listCapturableApplications(): Promise<CapturableApplication[]> {
    return this.state.applications ?? [];
  }

  public async listAudioDevices(): Promise<AudioDeviceDescriptor[]> {
    return this.state.audioDevices ?? [];
  }
}

function record(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

/** The mock has no vendor instance: its current document stands in for captured live state. */
function preserveMockProcessorState(current: RuntimeGraphDocument, request: GraphApplyRequest) {
  const { processorParameterChanges = [], ...document } = structuredClone(request);
  const previous = new Map(current.nodes.map((node) => [node.id, node]));
  const seen = new Set<string>();
  for (const change of processorParameterChanges) {
    const old = previous.get(change.nodeId);
    const next = document.nodes.find((node) => node.id === change.nodeId);
    const key = JSON.stringify([change.nodeId, change.parameterId]);
    if (
      seen.has(key) ||
      old?.type !== 'provider' ||
      next?.type !== 'provider' ||
      record(old.config).typeId !== change.typeId ||
      record(next.config).typeId !== change.typeId ||
      !Number.isInteger(change.parameterId) ||
      change.parameterId < 0 ||
      !Number.isFinite(change.value) ||
      change.value < 0 ||
      change.value > 1
    )
      throw new Error('graph_invalid');
    seen.add(key);
  }
  if (processorParameterChanges.length > 4096) throw new Error('graph_invalid');
  document.nodes = document.nodes.map((node) => {
    const old = previous.get(node.id);
    const config = record(node.config);
    const oldConfig = record(old?.config);
    if (node.type !== 'provider' || old?.type !== 'provider' || config.typeId !== oldConfig.typeId)
      return node;
    const parameters = record(oldConfig.parameters);
    const values = { ...record(parameters.parameterValues) };
    for (const change of processorParameterChanges)
      if (change.nodeId === node.id) values[String(change.parameterId)] = change.value;
    return {
      ...node,
      config: { ...config, parameters: { ...parameters, parameterValues: values } },
    };
  });
  return document;
}

function closedEditor(nodeId: string): ProcessorEditorStatus {
  return {
    failure: null,
    height: null,
    nodeId,
    protocolVersion: 1,
    resizable: false,
    state: 'closed',
    width: null,
  };
}
