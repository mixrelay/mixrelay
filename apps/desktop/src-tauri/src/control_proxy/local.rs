//! Local controllers use the same capture-aware command boundary as the desktop UI.
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use mixrelay_control_protocol::{
    AppStatus, GraphCatalogResponse, GraphNodeControls, GraphRuntimeStatus, NodeControlRequest,
    RecorderControlRequest,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ControlProxyClient, ControlProxyResponse, Method, node_control, recorder, rejected};
use crate::macos_audio::ApplicationCaptureState;

pub(crate) mod socket;

/// One request per connection. Callers refresh after any disconnect and never replay writes.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub id: u64,
    command: Command,
}

impl Request {
    pub(super) fn is_read(&self) -> bool {
        matches!(self.command, Command::Snapshot {})
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Snapshot {},
    Node {
        session: String,
        target: Target,
        request: NodeControlRequest,
    },
    Recorder {
        session: String,
        target: Target,
        request: RecorderControlRequest,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Target {
    graph_id: String,
    node_id: String,
}

pub(crate) struct LocalControl {
    client: ControlProxyClient,
    captures: Arc<ApplicationCaptureState>,
    session: String,
}

impl LocalControl {
    pub(crate) fn new(
        client: ControlProxyClient,
        captures: Arc<ApplicationCaptureState>,
    ) -> Result<Self, String> {
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            client,
            captures,
            session: format!("{}-{}", std::process::id(), started.as_nanos()),
        })
    }

    pub(crate) async fn dispatch(&self, request: Request) -> Value {
        let response = self
            .execute(request.command)
            .await
            .unwrap_or_else(|reason| ControlProxyResponse {
                status: 503,
                body: json!({"code":"control_unavailable", "parameters":{"reason":reason}}),
                delivery: super::RequestDelivery::Uncertain,
            });
        json!({"id":request.id,"status":response.status,"body":response.body})
    }

    async fn execute(&self, command: Command) -> Result<ControlProxyResponse, String> {
        match command {
            Command::Snapshot {} => self.snapshot().await,
            Command::Node {
                session,
                target,
                request,
            } => {
                if session != self.session {
                    return Ok(rejected("graph_revision_stale"));
                }
                self.client
                    .update_node_control(
                        &self.captures,
                        &target.graph_id,
                        &target.node_id,
                        serde_json::to_value(request).map_err(|error| error.to_string())?,
                    )
                    .await
            }
            Command::Recorder {
                session,
                target,
                request,
            } => {
                if session != self.session || request.expected_recording.is_none() {
                    return Ok(rejected("graph_revision_stale"));
                }
                self.client
                    .request(
                        Method::PUT,
                        &recorder::route(&target.graph_id, &target.node_id),
                        Some(serde_json::to_value(request).map_err(|error| error.to_string())?),
                    )
                    .await
            }
        }
    }

    async fn read<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let response = self.client.request(Method::GET, path, None).await?;
        if !response.is_success() {
            return Err(format!("snapshot unavailable: {}", response.status));
        }
        serde_json::from_value(response.body).map_err(|error| error.to_string())
    }

    async fn snapshot(&self) -> Result<ControlProxyResponse, String> {
        let before: GraphRuntimeStatus = self.read("/api/v1/graph/status").await?;
        let catalog: GraphCatalogResponse = self.read(super::GRAPH_CATALOG_ROUTE).await?;
        if before.active_revision != before.submitted_revision {
            return Ok(rejected("graph_exchange_busy"));
        }
        let mut graphs = Vec::with_capacity(catalog.graphs.len());
        for graph in &catalog.graphs {
            let controls: GraphNodeControls =
                self.read(&node_control::route(&graph.id, None)?).await?;
            if controls.revision != before.submitted_revision {
                return Ok(rejected("graph_revision_stale"));
            }
            let mut nodes = Vec::with_capacity(controls.nodes.len());
            for node in controls.nodes {
                let recording = if node.capabilities.recorder {
                    Some(
                        self.read::<mixrelay_control_protocol::RecorderStatus>(&recorder::route(
                            &graph.id,
                            &node.node_id,
                        ))
                        .await?,
                    )
                } else {
                    None
                };
                let mut value = serde_json::to_value(node).map_err(|error| error.to_string())?;
                if let Some(recording) = recording {
                    value["recorder"] = json!(recording);
                }
                nodes.push(value);
            }
            graphs.push(json!({"id":graph.id,"name":graph.name,"nodes":nodes}));
        }
        let after: GraphRuntimeStatus = self.read("/api/v1/graph/status").await?;
        let current: GraphCatalogResponse = self.read(super::GRAPH_CATALOG_ROUTE).await?;
        if before != after || catalog.catalog_revision != current.catalog_revision {
            return Ok(rejected("graph_revision_stale"));
        }
        let status: AppStatus = self.read("/api/v1/status").await?;
        Ok(ControlProxyResponse {
            status: 200,
            delivery: super::RequestDelivery::ServerResponse,
            body: json!({"protocolVersion":1,"session":self.session,"revision":after.active_revision,"enginePower":status.engine_power,"graphs":graphs}),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_bounded_commands_deserialize() {
        for value in [
            json!({"id":1,"command":{"type":"snapshot","url":"/private"}}),
            json!({"id":1,"command":{"type":"toggle"}}),
            json!({"id":1,"command":{"type":"node","session":"s","target":{"graphId":"g","nodeId":"n"},"request":{"enabled":true}}}),
        ] {
            assert!(serde_json::from_value::<Request>(value).is_err());
        }
        assert!(
            serde_json::from_value::<Request>(json!({"id":1,"command":{"type":"snapshot"}}))
                .expect("snapshot")
                .is_read()
        );
    }

    #[tokio::test]
    async fn previous_session_and_unfenced_recorder_never_reach_engine() {
        let control =
            LocalControl::new(ControlProxyClient::default(), Arc::default()).expect("control");
        for (kind, session, request) in [
            (
                "node",
                "previous",
                json!({"expectedRevision":1,"enabled":false}),
            ),
            (
                "recorder",
                control.session.as_str(),
                json!({"expectedRevision":1,"request":{"action":"stop"}}),
            ),
        ] {
            let request = serde_json::from_value(json!({"id":9,"command":{"type":kind,"session":session,"target":{"graphId":"g","nodeId":"n"},"request":request}})).expect("request");
            let response = control.dispatch(request).await;
            assert_eq!(response["status"], 409);
            assert_eq!(
                response["body"]["parameters"]["nodeControlOutcome"],
                "not_applied"
            );
        }
    }
}
