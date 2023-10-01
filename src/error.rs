use std::io;

use  thiserror::Error;

#[derive(Error, Debug)]
pub enum UringError {

    #[cfg(target_os = "linux")]
    #[error("Failed to init rings: {0}")]
    FailedInit(io::Error),

    #[cfg(target_os = "linux")]
    #[error("Feature [{0}] required but unsupported by current kernel")]
    UnsupportedFeature(&'static str),

    #[cfg(target_os = "linux")]
    #[error("Failed Opcode Probes: {0}")]
    ProbeFailed(io::Error),

    #[cfg(target_os = "linux")]
    #[error("Opcode [{0}] required, but not supported by current kernel ")]
    UnsupportedOpcode(&'static str),

    #[cfg(target_os = "linux")]
    #[error("Failed to submit IO: {0}")]
    SubmitFailed(io::Error),

    #[error("Generic IO Error")]
    IOError(io::Error),

}