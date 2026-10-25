//! Typed editor-to-runtime failures retain the failing stage and delivery uncertainty.
use std::io;

use mixrelay_control_protocol::{
    ControlBoundary, ControlFailureCode, ControlFailureContext, ControlStage, FailureDelivery,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ConnectionFailure {
    #[error("could not resolve the editor home directory")]
    HomeDirectory(#[source] tauri::Error),
    #[error("could not inspect the editor home directory")]
    HomeMetadata(#[source] io::Error),
    #[error("could not resolve the runtime control directory")]
    ControlDirectory(#[source] io::Error),
    #[error("background connection timed out")]
    ConnectTimeout,
    #[error("could not connect to the background controller: {0}")]
    Connect(#[source] io::Error),
    #[error("could not verify background controller credentials")]
    PeerCredentials(#[source] io::Error),
    #[error("background controller belongs to a different user")]
    PeerMismatch,
    #[error("background connection closed")]
    ClosedBeforeAdmission,
    #[error("background request is too large")]
    RequestTooLarge,
    #[error("background connection unavailable")]
    SendFailed,
    #[error("background disconnected; command was not replayed")]
    DisconnectedAfterAdmission,
    #[error("background response is uncertain; command was not replayed")]
    ResponseTimeout,
    #[error("background controller unavailable")]
    ControllerUnavailable,
    #[error("invalid background result")]
    InvalidResponse,
    #[error("{0}")]
    SubscriptionRejected(String),
    #[error("meter subscription timed out")]
    SubscriptionTimeout,
}

impl ConnectionFailure {
    pub(super) fn diagnostic(&self) -> ControlFailureContext {
        ControlFailureContext {
            platform: crate::control_channel::failure_platform(),
            boundary: ControlBoundary::EditorRuntime,
            stage: self.stage(),
            failure: self.code(),
            delivery: self.delivery(),
            os_error: self.os_error(),
        }
    }

    fn stage(&self) -> ControlStage {
        match self {
            Self::HomeDirectory(_) | Self::HomeMetadata(_) | Self::ControlDirectory(_) => {
                ControlStage::Setup
            }
            Self::ConnectTimeout | Self::Connect(_) => ControlStage::Connect,
            Self::PeerCredentials(_) | Self::PeerMismatch => ControlStage::PeerVerification,
            Self::ClosedBeforeAdmission => ControlStage::Admission,
            Self::RequestTooLarge => ControlStage::Encode,
            Self::SendFailed => ControlStage::Send,
            Self::DisconnectedAfterAdmission
            | Self::ResponseTimeout
            | Self::ControllerUnavailable => ControlStage::Response,
            Self::InvalidResponse => ControlStage::Decode,
            Self::SubscriptionRejected(_) | Self::SubscriptionTimeout => ControlStage::Subscribe,
        }
    }

    fn code(&self) -> ControlFailureCode {
        match self {
            Self::HomeDirectory(_) => ControlFailureCode::HomeDirectory,
            Self::HomeMetadata(_) => ControlFailureCode::HomeMetadata,
            Self::ControlDirectory(_) => ControlFailureCode::ControlDirectory,
            Self::ConnectTimeout => ControlFailureCode::ConnectTimeout,
            Self::Connect(_) => ControlFailureCode::ConnectIo,
            Self::PeerCredentials(_) => ControlFailureCode::PeerCredentials,
            Self::PeerMismatch => ControlFailureCode::PeerMismatch,
            Self::ClosedBeforeAdmission => ControlFailureCode::ClosedBeforeAdmission,
            Self::RequestTooLarge => ControlFailureCode::RequestTooLarge,
            Self::SendFailed => ControlFailureCode::SendFailed,
            Self::DisconnectedAfterAdmission => ControlFailureCode::DisconnectedAfterAdmission,
            Self::ResponseTimeout => ControlFailureCode::ResponseTimeout,
            Self::ControllerUnavailable => ControlFailureCode::ControllerUnavailable,
            Self::InvalidResponse => ControlFailureCode::InvalidResponse,
            Self::SubscriptionRejected(_) => ControlFailureCode::SubscriptionRejected,
            Self::SubscriptionTimeout => ControlFailureCode::SubscriptionTimeout,
        }
    }

    fn delivery(&self) -> FailureDelivery {
        match self {
            Self::HomeDirectory(_)
            | Self::HomeMetadata(_)
            | Self::ControlDirectory(_)
            | Self::ConnectTimeout
            | Self::Connect(_)
            | Self::PeerCredentials(_)
            | Self::PeerMismatch
            | Self::ClosedBeforeAdmission
            | Self::RequestTooLarge
            | Self::SendFailed => FailureDelivery::NotSent,
            Self::DisconnectedAfterAdmission
            | Self::ResponseTimeout
            | Self::ControllerUnavailable
            | Self::InvalidResponse
            | Self::SubscriptionRejected(_)
            | Self::SubscriptionTimeout => FailureDelivery::Uncertain,
        }
    }

    fn os_error(&self) -> Option<i32> {
        match self {
            Self::HomeMetadata(error)
            | Self::ControlDirectory(error)
            | Self::Connect(error)
            | Self::PeerCredentials(error) => error.raw_os_error(),
            _ => None,
        }
    }

    /// This classification permits only another catalog read. It never authorizes a mutation.
    pub(super) fn catalog_read_retryable(&self) -> bool {
        match self {
            Self::ConnectTimeout
            | Self::ClosedBeforeAdmission
            | Self::SendFailed
            | Self::DisconnectedAfterAdmission
            | Self::ResponseTimeout
            | Self::ControllerUnavailable => true,
            Self::Connect(error) => matches!(
                error.kind(),
                io::ErrorKind::NotFound
                    | io::ErrorKind::ConnectionRefused
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::TimedOut
            ),
            _ => false,
        }
    }
}
