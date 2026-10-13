import { Channel, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import {
  type AudioDeviceDescriptor,
  type CapturableApplication,
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
  type MeterTelemetry,
  type MeterTelemetryListener,
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

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
type MeterMessage = MeterTelemetry | { ended: true };
/** The native side keeps its end; the webview only receives. */
type MessageChannel = { onmessage: (message: MeterMessage) => void };

interface ControlProxyResponse {
  body: unknown;
  status: number;
}

export class TauriControlPort implements ControlPort {
  public getNodeControls(graphId: string): Promise<import('./types').GraphNodeControls> {
    return this.call('control_get_node_controls', { graphId });
  }
  public updateNodeControl(
    graphId: string,
    nodeId: string,
    request: import('./types').NodeControlRequest,
  ): Promise<NodeControlResponse> {
    return this.call('control_update_node_control', { graphId, nodeId, request });
  }
  public setEnginePower(enabled: boolean): Promise<GraphApplyResponse> {
    return this.call('control_set_engine_power', { enabled });
  }
  public onWorkspaceFile(
    listener: (action: 'new' | 'open' | 'save' | 'save_as') => void,
  ): Promise<() => void> {
    return listen<string>('workspace-file', ({ payload }) => {
      if (payload === 'new' || payload === 'open' || payload === 'save' || payload === 'save_as')
        listener(payload);
    });
  }

  public async workspaceFile(
    request: Parameters<NonNullable<ControlPort['workspaceFile']>>[0],
  ): Promise<GraphCatalog | null> {
    const response = await this.invokeCommand<ControlProxyResponse | null>('workspace_file', {
      request,
    });
    if (!response) return null;
    if (response.status < 200 || response.status >= 300)
      throw new ControlPortError(response.status, response.body as ProtocolErrorBody);
    return response.body as GraphCatalog;
  }
  public async recordingSettings(graphId: string, nodeId: string): Promise<RecorderSettings> {
    try {
      return await this.invokeCommand<RecorderSettings>('recording_settings', { graphId, nodeId });
    } catch (error) {
      throw invocationError('recording_settings', error);
    }
  }

  public async setRecordingSettings(
    graphId: string,
    nodeId: string,
    settings: RecorderSettings,
    expectedRevision: number,
  ): Promise<RecorderSettings> {
    try {
      return await this.invokeCommand<RecorderSettings>('save_recording_settings', {
        graphId,
        nodeId,
        settings,
        expectedRevision,
      });
    } catch (error) {
      throw invocationError('save_recording_settings', error);
    }
  }

  public recorderStatus(graphId: string, nodeId: string): Promise<RecorderStatus> {
    return this.call('control_get_recorder_status', { graphId, nodeId });
  }

  public controlRecorder(
    graphId: string,
    nodeId: string,
    request: RecorderRequest,
    expectedRevision: number,
  ): Promise<RecorderStatus> {
    return this.call('control_set_recorder', {
      graphId,
      nodeId,
      request: { expectedRevision, request },
    });
  }

  public async selectRecordingFolder(options: {
    defaultPath?: string;
    title?: string;
  }): Promise<string | null> {
    try {
      return await this.invokeCommand<string | null>('select_recording_folder', options);
    } catch (error) {
      throw invocationError('select_recording_folder', error);
    }
  }

  public setVirtualEndpointSampleRate(
    request: VirtualEndpointFormatRequest,
  ): Promise<GraphApplyResponse> {
    return this.call('control_set_virtual_endpoint_sample_rate', { request });
  }
  public getGraphLayout(graphId: string): Promise<GraphLayoutResponse> {
    return this.call('control_get_graph_layout', { graphId });
  }

  public setGraphLayout(
    graphId: string,
    request: GraphLayoutRequest,
  ): Promise<GraphLayoutResponse> {
    return this.call('control_set_graph_layout', { graphId, request });
  }

  public getGraphCatalog(): Promise<GraphCatalog> {
    return this.call('control_get_graph_catalog');
  }

  public mutateGraphCatalog(request: GraphCatalogRequest): Promise<GraphCatalog> {
    return this.call('control_mutate_graph_catalog', { request });
  }
  public reconfigureSession(request: {
    expectedRevision: number;
    format: { sampleRate: number; blockFrames: number };
  }): Promise<GraphApplyResponse> {
    return this.call('control_reconfigure_session', { request });
  }
  public setVirtualEndpointBufferFrames(
    request: Parameters<NonNullable<ControlPort['setVirtualEndpointBufferFrames']>>[0],
  ): Promise<GraphApplyResponse> {
    return this.call('control_set_virtual_endpoint_buffer_frames', { request });
  }
  public constructor(
    private readonly invokeCommand: Invoke = invoke,
    private readonly createChannel: () => MessageChannel = () => new Channel<MeterMessage>(),
  ) {}

  public applyGraph(document: GraphApplyRequest): Promise<GraphApplyResponse> {
    return this.call('control_apply_graph', { document });
  }

  public getGraph(): Promise<RuntimeGraphDocument> {
    return this.call('control_get_graph');
  }

  public getGraphStatus(): Promise<GraphRuntimeStatus> {
    return this.call('control_get_graph_status');
  }

  public getMeters(): Promise<MeterSnapshot> {
    return this.call('control_get_meters');
  }

  /** Each consumed window returns one credit, so a stalled webview receives nothing more
   * instead of accumulating messages natively. Windows can arrive before the id does. */
  public async subscribeMeters(listener: MeterTelemetryListener): Promise<() => void> {
    const channel = this.createChannel();
    let id: number | undefined;
    let owed = 0;
    let active = true;
    const credit = () => {
      if (id === undefined) owed += 1;
      else void this.invokeCommand('control_meter_credit', { id }).catch(() => undefined);
    };
    channel.onmessage = (message) => {
      // After unsubscribing, nothing is delivered and no credit returns, so even a lost
      // unsubscribe ends natively once credit runs out.
      if (!active) return;
      if ('ended' in message) return listener.ended();
      listener.window(message);
      credit();
    };
    id = await this.invokeCommand<number>('control_subscribe_meters', { channel });
    for (; owed > 0; owed -= 1) credit();
    return () => {
      active = false;
      void this.invokeCommand('control_unsubscribe_meters', { id }).catch(() => undefined);
    };
  }

  public getProcessors(): Promise<ExternalProcessorCatalog> {
    return this.call('control_get_processors');
  }

  public retryProcessor(request: ProcessorRetryRequest): Promise<GraphApplyResponse> {
    return this.call('control_retry_processor', { request });
  }

  public getProcessorEditor(nodeId: string): Promise<ProcessorEditorStatus> {
    return this.call('control_get_processor_editor', { nodeId });
  }

  public getProcessorEditorFrame(nodeId: string): Promise<ProcessorEditorFrame> {
    return this.call('control_get_processor_editor_frame', { nodeId });
  }

  public sendProcessorEditorInput(nodeId: string, input: ProcessorEditorInput): Promise<void> {
    return this.call('control_send_processor_editor_input', { nodeId, input });
  }

  public rescanProcessors(paths: string[]): Promise<ExternalProcessorCatalog> {
    return this.call('control_rescan_processors', { paths });
  }

  public async selectProcessorDirectory(): Promise<string | null> {
    try {
      return await this.invokeCommand<string | null>('select_processor_directory');
    } catch (error) {
      throw invocationError('select_processor_directory', error);
    }
  }

  public getStatus(): Promise<MixRelayStatus> {
    return this.call('control_get_status');
  }

  public setProcessorParameter(
    nodeId: string,
    parameterId: number,
    value: number,
    expectedRevision: number,
  ): Promise<ProcessorParameterUpdateResponse> {
    return this.call('control_set_processor_parameter', {
      nodeId,
      parameterId,
      value,
      expectedRevision,
    });
  }

  public setProcessorEditor(
    nodeId: string,
    open: boolean,
    expectedRevision: number,
  ): Promise<ProcessorEditorStatus> {
    return this.call('control_set_processor_editor', { nodeId, open, expectedRevision });
  }

  public async listCapturableApplications(): Promise<CapturableApplication[]> {
    try {
      return await this.invokeCommand<CapturableApplication[]>('list_capturable_applications');
    } catch (error) {
      throw invocationError('list_capturable_applications', error);
    }
  }

  public async listAudioDevices(): Promise<AudioDeviceDescriptor[]> {
    try {
      return await this.invokeCommand<AudioDeviceDescriptor[]>('list_audio_devices');
    } catch (error) {
      throw invocationError('list_audio_devices', error);
    }
  }

  private async call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    let response: ControlProxyResponse;
    try {
      response = await this.invokeCommand<ControlProxyResponse>(command, args);
    } catch (error) {
      throw invocationError(command, error);
    }
    if (response.status < 200 || response.status >= 300) {
      throw new ControlPortError(response.status, response.body as ProtocolErrorBody);
    }
    return response.body as T;
  }
}

function invocationError(command: string, cause: unknown): ControlPortError {
  // IPC rejection can mean an unknown command or invalid payload while the engine is healthy.
  // Only the native HTTP boundary can identify an actual engine connection failure.
  const reason = cause instanceof Error ? cause.message : String(cause);
  console.error(`Tauri command ${command} failed`, cause);
  return new ControlPortError(500, {
    code: 'control_request_failed',
    parameters: { command, reason },
  });
}
