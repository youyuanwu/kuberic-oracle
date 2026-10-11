use std::path::PathBuf;

use crate::poc::{PocCategory, PocFailure, PocStage};

pub const IMAGE: &str = "container-registry.oracle.com/database/free@sha256:c4d9b15a1b2240ce90730c23ce298fcdd34f6793792c1b3849181a080009c322";

#[derive(Debug, Clone)]
pub struct FixtureConfig {
    pub(crate) docker_program: PathBuf,
    pub(crate) docker_host: String,
    pub(crate) root: PathBuf,
    pub(crate) startup_timeout_seconds: u64,
}

impl FixtureConfig {
    pub fn from_environment() -> Result<Self, PocFailure> {
        if std::env::var("KUBERIC_ORACLE_ACCEPT_FREE_TERMS").as_deref() != Ok("YES") {
            return Err(PocFailure::new(
                PocStage::Fixture,
                PocCategory::Fixture,
                "Oracle Free terms authorization is required",
            ));
        }
        let docker_host = std::env::var("KUBERIC_ORACLE_DOCKER_HOST")
            .unwrap_or_else(|_| "unix:///var/run/docker.sock".to_owned());
        let docker_socket = docker_host
            .strip_prefix("unix://")
            .map(PathBuf::from)
            .filter(|path| {
                path.is_absolute()
                    && !path.components().any(|component| {
                        matches!(
                            component,
                            std::path::Component::CurDir | std::path::Component::ParentDir
                        )
                    })
            })
            .ok_or_else(|| {
                PocFailure::new(
                    PocStage::Fixture,
                    PocCategory::Fixture,
                    "live PoC Docker endpoint must be a normalized absolute Unix socket",
                )
            })?;
        if docker_socket.as_os_str().is_empty() {
            return Err(PocFailure::new(
                PocStage::Fixture,
                PocCategory::Fixture,
                "live PoC Docker endpoint must be a normalized absolute Unix socket",
            ));
        }
        let root = std::env::var_os("KUBERIC_ORACLE_FIXTURE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(format!("/tmp/kuberic-oracle-poc-{}", unsafe {
                    libc::geteuid()
                }))
            });
        Ok(Self {
            docker_program: PathBuf::from("docker"),
            docker_host,
            root,
            startup_timeout_seconds: 900,
        })
    }
}
