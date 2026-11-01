export type ProtocolErrorCode =
  | 'feature_limit'
  | 'node_not_found'
  | 'node_disabled'
  | 'recorder_failed'
  | 'audio_session_unsupported'
  | 'audio_session_processor_state_unavailable'
  | 'audio_session_reconfigure_failed'
  | 'audio_device_disconnected'
  | 'audio_permission_pending'
  | 'audio_permission_denied'
  | 'audio_source_unavailable'
  | 'control_unavailable'
  | 'control_request_failed'
  | 'engine_replaced'
  | 'invalid_operation_id'
  | 'graph_binding_unsupported'
  | 'graph_exchange_busy'
  | 'graph_invalid'
  | 'graph_revision_stale'
  | 'processor_instance_stale'
  | 'processor_not_failed'
  | 'processor_unavailable';

export interface ProtocolErrorBody {
  code: ProtocolErrorCode;
  parameters: unknown;
}

export class ControlPortError extends Error {
  public constructor(
    public readonly status: number,
    public readonly body: ProtocolErrorBody,
  ) {
    super(body.code);
    this.name = 'ControlPortError';
  }
}
