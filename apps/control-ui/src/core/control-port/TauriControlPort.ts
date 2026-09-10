import { invoke } from '@tauri-apps/api/core';
import { recordingFileName } from './recordingFileName';

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
  type ProcessorEditorFrame,
  type ProcessorEditorInput,
  type ProcessorEditorStatus,
  type ProcessorParameterUpdateResponse,
  type ProtocolErrorBody,
  type RecorderRequest,
  type RecorderStatus,
  type RuntimeGraphDocument,
  type ToneNariStatus,
  type VirtualEndpointFormatRequest,
} from './types';

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

interface ControlProxyResponse {
  body: unknown;
  status: number;
}

export class TauriControlPort implements ControlPort {
  public async defaultRecordingPath(): Promise<string> {
    try {
      return await this.invokeCommand<string>('default_recording_path', {
        fileName: recordingFileName(),
      });
    } catch (error) {
      throw invocationError('default_recording_path', error);
    }
  }

  public recorderStatus(graphId: string, nodeId: string): Promise<RecorderStatus> {
    return this.call('control_get_recorder_status', { graphId, nodeId });
  }

  public controlRecorder(
    graphId: string,
    nodeId: string,
    request: RecorderRequest,
  ): Promise<RecorderStatus> {
    return this.call('control_set_recorder', { graphId, nodeId, request });
  }

  public async selectRecordingPath(options: {
    defaultPath?: string;
    title?: string;
  }): Promise<string | null> {
    try {
      return await this.invokeCommand<string | null>('select_recording_path', options);
    } catch (error) {
      throw invocationError('select_recording_path', error);
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
  public constructor(private readonly invokeCommand: Invoke = invoke) {}

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

  public getProcessors(): Promise<ExternalProcessorCatalog> {
    return this.call('control_get_processors');
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

  public getStatus(): Promise<ToneNariStatus> {
    return this.call('control_get_status');
  }

  public setProcessorParameter(
    nodeId: string,
    parameterId: number,
    value: number,
  ): Promise<ProcessorParameterUpdateResponse> {
    return this.call('control_set_processor_parameter', { nodeId, parameterId, value });
  }

  public setProcessorEditor(nodeId: string, open: boolean): Promise<ProcessorEditorStatus> {
    return this.call('control_set_processor_editor', { nodeId, open });
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
