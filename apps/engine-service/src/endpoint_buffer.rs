//! Native client buffer edits are device settings, not DSP graph publications.
use super::{EngineGraphController, session_error};
use mixrelay_control_protocol::{
    ErrorCode, GraphApplyResponse, GraphApplyState, PROTOCOL_VERSION, ProtocolError,
    VirtualEndpointBufferRequest,
};

impl EngineGraphController {
    pub(super) fn change_endpoint_buffer(
        &self,
        request: &VirtualEndpointBufferRequest,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        let _mutation = self.mutation.lock();
        self.require_active_revision(request.expected_revision)?;
        let actual = self
            .graph_resources
            .virtual_endpoint_buffer(&request.endpoint_id)
            .ok_or_else(|| unsupported(request))?;
        if !actual.accepts(request.buffer_frames) {
            return Err(unsupported(request));
        }
        if actual.current_frames != request.expected_buffer_frames
            || actual.preferred_frames != request.expected_preferred_frames
        {
            return Err(session_error(
                "virtual device I/O buffer changed; refresh its settings",
            ));
        }
        let persistence_error = self
            .graph_resources
            .set_virtual_endpoint_buffer(
                &request.endpoint_id,
                request.expected_buffer_frames,
                request.expected_preferred_frames,
                request.buffer_frames,
            )
            .map_err(|error| session_error(&error.to_string()))?;
        // Rebuilding the graph would needlessly reset meters, DSP and UI history.
        Ok(GraphApplyResponse {
            protocol_version: PROTOCOL_VERSION,
            revision: self.exchange.revisions().active,
            state: GraphApplyState::Active,
            persistence_error,
        })
    }
}

fn unsupported(request: &VirtualEndpointBufferRequest) -> ProtocolError {
    ProtocolError {
        code: ErrorCode::AudioSessionUnsupported,
        parameters: serde_json::json!({"resource":"virtual_endpoint_buffer",
            "endpointId":request.endpoint_id,"bufferFrames":request.buffer_frames}),
    }
}

#[cfg(test)]
mod tests;
