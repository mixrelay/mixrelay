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
  type NodeControlResponse,
  type ProcessorEditorFrame,
  type ProcessorEditorInput,
  type ProcessorEditorStatus,
  type ProcessorParameterUpdateResponse,
  type ProcessorRetryRequest,
  type ProtocolErrorBody,
  type RecorderRequest,
  type RecorderSettings,
  type RecorderStatus,
  type RuntimeGraphDocument,
  type VirtualEndpointFormatRequest,
} from './types';

// The engine binds IPv4 loopback explicitly. Using `localhost` can resolve to `::1` first and
// make the browser-only dev surface report a disconnected engine while Tauri remains healthy.
const LOCAL_CONTROL_ORIGIN = 'http://127.0.0.1:19341';
const OPERATION_HEADER = 'X-MixRelay-Operation';
const OPERATION_RESULT_HEADER = 'X-MixRelay-Operation-Result';
const OPERATION_POLL_INTERVAL_MS = 100;
const OPERATION_POLL_TIMEOUT_MS = 5000;
const OPERATION_ACK_TIMEOUT_MS = 1000;
const MAX_OPERATION_ID_LENGTH = 128;

export class HttpControlPort implements ControlPort {
  public async recordingSettings(_graphId: string, _nodeId: string): Promise<RecorderSettings> {
    throw new ControlPortError(501, {
      code: 'control_request_failed',
      parameters: { reason: 'Recording settings require the desktop application.' },
    });
  }

  public async setRecordingSettings(
    graphId: string,
    nodeId: string,
    _settings: RecorderSettings,
    _expectedRevision: number,
  ): Promise<RecorderSettings> {
    return this.recordingSettings(graphId, nodeId);
  }
  public getNodeControls(graphId: string): Promise<import('./types').GraphNodeControls> {
    return this.json(`/api/v1/graphs/${encodeURIComponent(graphId)}/nodes/controls`);
  }
  public updateNodeControl(
    graphId: string,
    nodeId: string,
    request: import('./types').NodeControlRequest,
  ): Promise<NodeControlResponse> {
    return this.json(
      `/api/v1/graphs/${encodeURIComponent(graphId)}/nodes/${encodeURIComponent(nodeId)}/control`,
      {
        method: 'PUT',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(request),
      },
    );
  }
  public recorderStatus(graphId: string, nodeId: string): Promise<RecorderStatus> {
    return this.json(
      `/api/v1/graphs/${encodeURIComponent(graphId)}/recorders/${encodeURIComponent(nodeId)}`,
    );
  }

  public controlRecorder(
    graphId: string,
    nodeId: string,
    request: RecorderRequest,
    expectedRevision: number,
  ): Promise<RecorderStatus> {
    return this.json(
      `/api/v1/graphs/${encodeURIComponent(graphId)}/recorders/${encodeURIComponent(nodeId)}`,
      {
        method: 'PUT',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ expectedRevision, request }),
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

  public retryProcessor(request: ProcessorRetryRequest): Promise<GraphApplyResponse> {
    return this.json('/api/v1/processors/retry', {
      body: JSON.stringify(request),
      headers: { 'content-type': 'application/json' },
      method: 'PUT',
    });
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
    expectedRevision: number,
  ): Promise<ProcessorParameterUpdateResponse> {
    return this.json(`/api/v1/processors/${encodeURIComponent(nodeId)}/parameters/${parameterId}`, {
      body: JSON.stringify({ value, expectedRevision }),
      headers: { 'content-type': 'application/json' },
      method: 'PUT',
    });
  }

  public setProcessorEditor(
    nodeId: string,
    open: boolean,
    expectedRevision: number,
  ): Promise<ProcessorEditorStatus> {
    return this.json(`/api/v1/processors/${encodeURIComponent(nodeId)}/editor`, {
      body: JSON.stringify({ open, expectedRevision }),
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
    const operationId = pendingOperationId(response);
    if (operationId !== undefined) {
      validateOperationReceipt(body, operationId);
      return this.pollOperation<T>(operationId);
    }
    if (!response.ok) {
      throw new ControlPortError(response.status, body as ProtocolErrorBody);
    }
    return body as T;
  }

  private async pollOperation<T>(operationId: string): Promise<T> {
    const path = `/api/v1/operations/${encodeURIComponent(operationId)}`;
    const request = this.request;
    for (;;) {
      await new Promise<void>((resolve) => setTimeout(resolve, OPERATION_POLL_INTERVAL_MS));
      const response = await request(`${this.origin}${path}`, {
        method: 'GET',
        signal: AbortSignal.timeout(OPERATION_POLL_TIMEOUT_MS),
      });
      const body: unknown = await response.json();
      const pendingId = pendingOperationId(response);
      if (pendingId !== undefined) {
        if (pendingId !== operationId) {
          throw new Error('local control operation identity changed or was invalid');
        }
        validateOperationReceipt(body, operationId);
        continue;
      }
      validateTerminalOperation(response, operationId);
      await this.acknowledgeOperation(path);
      if (!response.ok) {
        throw new ControlPortError(response.status, body as ProtocolErrorBody);
      }
      return body as T;
    }
  }

  private async acknowledgeOperation(path: string): Promise<void> {
    try {
      const request = this.request;
      await request(`${this.origin}${path}`, {
        method: 'DELETE',
        signal: AbortSignal.timeout(OPERATION_ACK_TIMEOUT_MS),
      });
    } catch {
      // The terminal response is already authoritative; acknowledgement only reclaims
      // the server-side receipt and must not replace the observed operation result.
    }
  }
}

function pendingOperationId(response: Response): string | undefined {
  const operationId = response.headers.get(OPERATION_HEADER);
  if (operationId === null) return undefined;
  if (
    response.status !== 202 ||
    operationId.length === 0 ||
    operationId.length > MAX_OPERATION_ID_LENGTH
  ) {
    throw new Error('invalid operation receipt');
  }
  return operationId;
}

function validateOperationReceipt(body: unknown, operationId: string): void {
  if (
    typeof body !== 'object' ||
    body === null ||
    !('operationId' in body) ||
    typeof body.operationId !== 'string' ||
    body.operationId !== operationId
  ) {
    throw new Error('local control operation identity changed or was invalid');
  }
}

function validateTerminalOperation(response: Response, operationId: string): void {
  if (response.headers.get(OPERATION_RESULT_HEADER) !== operationId) {
    throw new Error('local control operation result could not be confirmed');
  }
}
