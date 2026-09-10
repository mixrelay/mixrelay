import {
  type ControlPort,
  ControlPortError,
  type ExternalProcessorCatalog,
  type GraphApplyRequest,
  type GraphApplyResponse,
  type GraphCatalog,
  type GraphCatalogRequest,
  type GraphLayoutRequest,
  type GraphLayoutResponse,
  type GraphRuntimeStatus,
  type MeterSnapshot,
  type MixRelayStatus,
  type ProcessorEditorFrame,
  type ProcessorEditorInput,
  type ProcessorEditorStatus,
  type ProcessorParameterUpdateResponse,
  type ProtocolErrorBody,
  type RecorderRequest,
  type RecorderStatus,
  type RuntimeGraphDocument,
  type VirtualEndpointFormatRequest,
} from './types';

// The engine binds IPv4 loopback explicitly. Using `localhost` can resolve to `::1` first and
// make the browser-only dev surface report a disconnected engine while Tauri remains healthy.
const LOCAL_CONTROL_ORIGIN = 'http://127.0.0.1:19341';

export class HttpControlPort implements ControlPort {
  public recorderStatus(graphId: string, nodeId: string): Promise<RecorderStatus> {
    return this.json(
      `/api/v1/graphs/${encodeURIComponent(graphId)}/recorders/${encodeURIComponent(nodeId)}`,
    );
  }

  public controlRecorder(
    graphId: string,
    nodeId: string,
    request: RecorderRequest,
  ): Promise<RecorderStatus> {
    return this.json(
      `/api/v1/graphs/${encodeURIComponent(graphId)}/recorders/${encodeURIComponent(nodeId)}`,
      {
        method: 'PUT',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(request),
      },
    );
  }

  public setVirtualEndpointSampleRate(
    request: VirtualEndpointFormatRequest,
  ): Promise<GraphApplyResponse> {
    return this.json('/api/v1/audio/virtual-endpoint/format', {
      method: 'PUT',
      body: JSON.stringify(request),
      headers: { 'content-type': 'application/json' },
    });
  }
  public getGraphLayout(graphId: string): Promise<GraphLayoutResponse> {
    return this.json(`/api/v1/graphs/${encodeURIComponent(graphId)}/layout`);
  }

  public setGraphLayout(
    graphId: string,
    request: GraphLayoutRequest,
  ): Promise<GraphLayoutResponse> {
    return this.json(`/api/v1/graphs/${encodeURIComponent(graphId)}/layout`, {
      body: JSON.stringify(request),
      headers: { 'content-type': 'application/json' },
      method: 'PUT',
    });
  }

  public getGraphCatalog(): Promise<GraphCatalog> {
    return this.json('/api/v1/graphs');
  }

  public mutateGraphCatalog(request: GraphCatalogRequest): Promise<GraphCatalog> {
    return this.json('/api/v1/graphs', {
      body: JSON.stringify(request),
      headers: { 'content-type': 'application/json' },
      method: 'PUT',
    });
  }
  public reconfigureSession(request: {
    expectedRevision: number;
    format: { sampleRate: number; blockFrames: number };
  }): Promise<GraphApplyResponse> {
    return this.json('/api/v1/audio/session', {
      body: JSON.stringify(request),
      headers: { 'content-type': 'application/json' },
      method: 'PUT',
    });
  }
  public constructor(
    private readonly origin = LOCAL_CONTROL_ORIGIN,
    private readonly request: typeof fetch = fetch,
  ) {}

  public applyGraph(document: GraphApplyRequest): Promise<GraphApplyResponse> {
    return this.json('/api/v1/graph', {
      body: JSON.stringify(document),
      headers: { 'content-type': 'application/json' },
      method: 'PUT',
    });
  }

  public getGraph(): Promise<RuntimeGraphDocument> {
    return this.json('/api/v1/graph');
  }

  public getGraphStatus(): Promise<GraphRuntimeStatus> {
    return this.json('/api/v1/graph/status');
  }

  public getMeters(): Promise<MeterSnapshot> {
    return this.json('/api/v1/meters');
  }

  public getProcessors(): Promise<ExternalProcessorCatalog> {
    return this.json('/api/v1/processors');
  }

  public getProcessorEditor(nodeId: string): Promise<ProcessorEditorStatus> {
    return this.json(`/api/v1/processors/${encodeURIComponent(nodeId)}/editor`);
  }

  public getProcessorEditorFrame(nodeId: string): Promise<ProcessorEditorFrame> {
    return this.json(`/api/v1/processors/${encodeURIComponent(nodeId)}/editor/frame`);
  }

  public sendProcessorEditorInput(nodeId: string, input: ProcessorEditorInput): Promise<void> {
    return this.json(`/api/v1/processors/${encodeURIComponent(nodeId)}/editor/input`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(input),
    });
  }

  public rescanProcessors(paths: string[]): Promise<ExternalProcessorCatalog> {
    return this.json('/api/v1/processors/scan', {
      body: JSON.stringify({ paths }),
      headers: { 'content-type': 'application/json' },
      method: 'PUT',
    });
  }

  public getStatus(): Promise<MixRelayStatus> {
    return this.json('/api/v1/status');
  }

  public setProcessorParameter(
    nodeId: string,
    parameterId: number,
    value: number,
  ): Promise<ProcessorParameterUpdateResponse> {
    return this.json(`/api/v1/processors/${encodeURIComponent(nodeId)}/parameters/${parameterId}`, {
      body: JSON.stringify({ value }),
      headers: { 'content-type': 'application/json' },
      method: 'PUT',
    });
  }

  public setProcessorEditor(nodeId: string, open: boolean): Promise<ProcessorEditorStatus> {
    return this.json(`/api/v1/processors/${encodeURIComponent(nodeId)}/editor`, {
      body: JSON.stringify({ open }),
      headers: { 'content-type': 'application/json' },
      method: 'PUT',
    });
  }

  private async json<T>(path: string, init?: RequestInit): Promise<T> {
    // Browser fetch is a Web IDL method, not an adapter method. Calling it with
    // this port as receiver throws before any HTTP request is sent in WebKit.
    const request = this.request;
    const response = await request(`${this.origin}${path}`, init);
    const body: unknown = await response.json();
    if (!response.ok) {
      throw new ControlPortError(response.status, body as ProtocolErrorBody);
    }
    return body as T;
  }
}
