use super::*;
use crate::{NodeControl, ProviderPorts};

fn dormant(ports: Option<ProviderPorts>) -> NodeSpec {
    NodeSpec {
        id: "missing".into(),
        label: None,
        control: NodeControl {
            enabled: false,
            muted: true,
        },
        config: NodeConfig::Provider(ProviderNodeConfig {
            type_id: "unavailable:fixture".into(),
            ports,
            parameters: serde_json::json!({"preserved":42}),
        }),
    }
}

#[test]
fn off_unavailable_provider_preserves_saved_ports_routes_and_controls() {
    let off = dormant(Some(ProviderPorts {
        inputs: 2,
        outputs: 2,
    }));
    let document = GraphDocument::new(
        1,
        vec![gain_node("source"), off.clone(), gain_node("sink")],
        vec![
            edge("in", "source", "missing"),
            edge("out", "missing", "sink"),
        ],
    );
    let execution = GraphCompiler::default()
        .compile(&document, context())
        .unwrap();
    let node = execution
        .nodes()
        .iter()
        .find(|node| node.id() == &off.id)
        .unwrap();
    assert_eq!(node.control(), off.control);
    assert_eq!(node.config(), &off.config);
    assert_eq!(node.inputs()[0].channels(), 2);
    assert_eq!(node.outputs()[0].channels(), 2);
    assert_eq!(execution.edges().len(), 2);
}

#[test]
fn off_provider_without_saved_ports_is_inert_and_on_still_requires_capability() {
    let mut document = GraphDocument::new(1, vec![dormant(None)], vec![]);
    let execution = GraphCompiler::default()
        .compile(&document, context())
        .unwrap();
    let node = &execution.nodes()[0];
    assert!(node.inputs().is_empty());
    assert!(node.outputs().is_empty());
    assert_eq!(node.config(), &document.nodes[0].config);
    for ports in [
        None,
        Some(ProviderPorts {
            inputs: 2,
            outputs: 2,
        }),
    ] {
        document.nodes[0] = dormant(ports);
        document.nodes[0].control.enabled = true;
        let error = GraphCompiler::default()
            .compile(&document, context())
            .unwrap_err();
        assert!(matches!(
            error.issues(),
            [GraphIssue::NodeResolutionFailed { .. }]
        ));
    }
}

#[test]
fn invalid_saved_ports_are_not_accepted_for_dormant_provider() {
    for (inputs, outputs) in [(0, 2), (2, 0), (65, 2), (2, 65)] {
        let document = GraphDocument::new(
            1,
            vec![dormant(Some(ProviderPorts { inputs, outputs }))],
            vec![],
        );
        let error = GraphCompiler::default()
            .compile(&document, context())
            .unwrap_err();
        assert!(matches!(
            error.issues(),
            [GraphIssue::NodeResolutionFailed { .. }]
        ));
    }
}

#[test]
fn dormant_provider_does_not_hide_configuration_errors_from_resolver() {
    struct Invalid;
    impl NodeResolver for Invalid {
        fn resolve(
            &self,
            _: &NodeSpec,
            _: &CompileContext,
        ) -> Result<ResolvedNode, NodeResolutionError> {
            Err(NodeResolutionError::InvalidConfiguration {
                reason: "invalid fixture state".into(),
            })
        }
    }
    let document = GraphDocument::new(
        1,
        vec![dormant(Some(ProviderPorts {
            inputs: 2,
            outputs: 2,
        }))],
        vec![],
    );
    let error = GraphCompiler::new(Invalid)
        .compile(&document, context())
        .unwrap_err();
    assert!(
        matches!(error.issues(), [GraphIssue::NodeResolutionFailed { reason, .. }]
        if reason.contains("invalid fixture state"))
    );
}
