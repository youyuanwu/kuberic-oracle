use std::{
    fs::OpenOptions,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::poc::{PocCategory, PocEndpoint, PocFailure, PocStage};

use super::{
    config::{FixtureConfig, IMAGE},
    credentials::FixtureCredentials,
    owner::{FileIdentity, OwnerClaim, OwnershipJournal},
};

const NETWORK_NAME: &str = "kuberic-oracle-poc-network";
const VOLUME_NAME: &str = "kuberic-oracle-poc-volume";
const CONTAINER_NAME: &str = "kuberic-oracle-poc-database";
const OBSERVER: &str = "KUBERIC_OBSERVER";

pub struct OracleFixture {
    config: FixtureConfig,
    owner: Option<OwnerClaim>,
    journal: OwnershipJournal,
    journal_identity: Option<FileIdentity>,
    credentials: Option<FixtureCredentials>,
    database_id: u64,
    database_unique_name: String,
}

impl OracleFixture {
    pub fn start(config: FixtureConfig) -> Result<Self, PocFailure> {
        let owner = OwnerClaim::acquire(&config.root)?;
        let token = owner.token.clone();
        let mut fixture = Self {
            config,
            owner: Some(owner),
            journal: OwnershipJournal {
                token,
                network_name: NETWORK_NAME.to_owned(),
                volume_name: VOLUME_NAME.to_owned(),
                container_name: CONTAINER_NAME.to_owned(),
                image: IMAGE.to_owned(),
                ..OwnershipJournal::default()
            },
            journal_identity: None,
            credentials: None,
            database_id: 0,
            database_unique_name: String::new(),
        };
        if let Err(operation) = fixture.start_inner() {
            let cleanup = fixture.cleanup_inner().err();
            return Err(match cleanup {
                Some(cleanup) => operation.with_cleanup(cleanup),
                None => operation,
            });
        }
        Ok(fixture)
    }

    pub fn endpoint(&self) -> PocEndpoint {
        PocEndpoint::loopback(self.journal.host_port, "FREEPDB1")
    }

    pub fn username_file(&self) -> &Path {
        &self.credentials.as_ref().unwrap().username_file
    }

    pub fn password_file(&self) -> &Path {
        &self.credentials.as_ref().unwrap().password_file
    }

    pub const fn database_id(&self) -> u64 {
        self.database_id
    }

    pub fn database_unique_name(&self) -> &str {
        &self.database_unique_name
    }

    pub fn create_wrong_password_file(&self) -> Result<(PathBuf, FileIdentity), PocFailure> {
        let path = self.config.root.join("wrong-password");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|_| fixture_failure("wrong-password file could not be created"))?;
        file.write_all(b"definitely-wrong-password")
            .and_then(|()| file.sync_all())
            .map_err(|_| fixture_failure("wrong-password file could not be written"))?;
        Ok((path.clone(), FileIdentity::from_path(&path)?))
    }

    pub fn connection_trace_path(&self) -> PathBuf {
        self.config.root.join("worker-connect.trace")
    }

    pub fn verify_loopback_trace(&self, path: &Path) -> Result<FileIdentity, PocFailure> {
        let contents = std::fs::read_to_string(path)
            .map_err(|_| fixture_failure("worker connection trace is unavailable"))?;
        let mut connect_count = 0usize;
        for line in contents.lines().filter(|line| {
            line.contains("connect(")
                && (line.contains("sa_family=AF_INET") || line.contains("sa_family=AF_INET6"))
        }) {
            connect_count += 1;
            if !line.contains("127.0.0.1") {
                return Err(fixture_failure(
                    "worker connection escaped the loopback fixture boundary",
                ));
            }
        }
        if connect_count == 0 {
            return Err(fixture_failure(
                "worker connection trace contained no network connection",
            ));
        }
        FileIdentity::from_path(path)
    }

    pub fn remove_connection_trace(&self, path: &Path) -> Result<(), PocFailure> {
        if !path.starts_with(&self.config.root) {
            return Err(cleanup_failure(
                "worker trace path escaped the fixture root",
            ));
        }
        match FileIdentity::from_path(path) {
            Ok(identity) => self.remove_owned_file(path, identity),
            Err(_) if !path.exists() => Ok(()),
            Err(_) => Err(cleanup_failure("worker trace identity is invalid")),
        }
    }

    pub fn remove_owned_file(&self, path: &Path, identity: FileIdentity) -> Result<(), PocFailure> {
        if !identity.matches_path(path) {
            return Err(cleanup_failure("owned temporary file identity changed"));
        }
        std::fs::remove_file(path)
            .map_err(|_| cleanup_failure("owned temporary file could not be removed"))
    }

    pub fn revoke_view(&self, view: &str) -> Result<(), PocFailure> {
        if !matches!(view, "SYS.V_$DATABASE" | "SYS.V_$INSTANCE") {
            return Err(fixture_failure("unsupported fixture grant mutation"));
        }
        self.execute_sql(&format!(
            "ALTER SESSION SET CONTAINER=FREEPDB1;\nREVOKE SELECT ON {view} FROM {OBSERVER};"
        ))
    }

    pub fn grant_view(&self, view: &str) -> Result<(), PocFailure> {
        if !matches!(view, "SYS.V_$DATABASE" | "SYS.V_$INSTANCE") {
            return Err(fixture_failure("unsupported fixture grant mutation"));
        }
        self.execute_sql(&format!(
            "ALTER SESSION SET CONTAINER=FREEPDB1;\nGRANT SELECT ON {view} TO {OBSERVER};"
        ))
    }

    pub fn verify_privileges(&self) -> Result<(), PocFailure> {
        let result = self.query_scalar(
            "ALTER SESSION SET CONTAINER=FREEPDB1;",
            &format!(
                "SELECT \
                 (SELECT COUNT(*) FROM DBA_ROLE_PRIVS WHERE GRANTEE='{OBSERVER}') || ':' || \
                 (SELECT COUNT(*) FROM DBA_SYS_PRIVS WHERE GRANTEE='{OBSERVER}' AND PRIVILEGE <> 'CREATE SESSION') || ':' || \
                 (SELECT COUNT(*) FROM DBA_TAB_PRIVS WHERE GRANTEE='{OBSERVER}' AND NOT (PRIVILEGE='SELECT' AND OWNER='SYS' AND TABLE_NAME IN ('V_$DATABASE','V_$INSTANCE'))) || ':' || \
                 (SELECT COUNT(*) FROM V$PWFILE_USERS WHERE USERNAME='{OBSERVER}') \
                 FROM DUAL"
            ),
        )?;
        if result != "0:0:0:0" {
            return Err(fixture_failure(
                "observer effective privileges exceed the PoC profile",
            ));
        }
        Ok(())
    }

    pub fn cleanup(mut self) -> Result<(), PocFailure> {
        self.cleanup_inner()
    }

    fn start_inner(&mut self) -> Result<(), PocFailure> {
        self.preflight()?;
        let credentials = FixtureCredentials::create(&self.config.root)?;
        self.journal.username_file = Some((
            credentials.username_file.clone(),
            credentials.username_identity,
        ));
        self.journal.password_file = Some((
            credentials.password_file.clone(),
            credentials.password_identity,
        ));
        self.journal.admin_env_file = Some((
            credentials.admin_env_file.clone(),
            credentials.admin_env_identity,
        ));
        self.credentials = Some(credentials);
        self.persist_journal()?;

        self.journal.network_id = self.docker(&[
            "network",
            "create",
            "--label",
            &format!("kuberic.oracle.owner={}", self.journal.token),
            NETWORK_NAME,
        ])?;
        self.persist_journal()?;

        self.docker(&[
            "volume",
            "create",
            "--label",
            &format!("kuberic.oracle.owner={}", self.journal.token),
            VOLUME_NAME,
        ])?;
        self.journal.volume_driver =
            self.docker(&["volume", "inspect", VOLUME_NAME, "--format", "{{.Driver}}"])?;
        self.persist_journal()?;

        let credentials = self.credentials.as_ref().unwrap();
        self.journal.container_id = self.docker(&[
            "run",
            "-d",
            "--name",
            CONTAINER_NAME,
            "--label",
            &format!("kuberic.oracle.owner={}", self.journal.token),
            "--network",
            NETWORK_NAME,
            "--volume",
            &format!("{VOLUME_NAME}:/opt/oracle/oradata"),
            "--publish",
            "127.0.0.1::1521",
            "--env-file",
            credentials.admin_env_file.to_str().unwrap(),
            IMAGE,
        ])?;
        self.persist_journal()?;
        self.wait_ready()?;
        self.journal.host_port = self.discover_port()?;
        self.persist_journal()?;
        self.verify_network_ownership()?;
        self.provision_observer()?;
        self.verify_privileges()?;
        let identity = self.query_scalar(
            "ALTER SESSION SET CONTAINER=FREEPDB1;",
            "SELECT DBID || ':' || DB_UNIQUE_NAME FROM V$DATABASE",
        )?;
        let (database_id, unique_name) = identity
            .split_once(':')
            .ok_or_else(|| fixture_failure("database identity query was malformed"))?;
        self.database_id = database_id
            .parse()
            .map_err(|_| fixture_failure("database ID query was malformed"))?;
        self.database_unique_name = unique_name.to_owned();
        Ok(())
    }

    fn preflight(&self) -> Result<(), PocFailure> {
        if std::env::consts::ARCH != "x86_64" || std::env::consts::OS != "linux" {
            return Err(fixture_failure("live PoC requires Linux x86-64"));
        }
        let architecture = self.docker(&["info", "--format", "{{.Architecture}}"])?;
        if architecture != "x86_64" {
            return Err(fixture_failure("Docker architecture is unsupported"));
        }
        for (kind, name) in [
            ("container", CONTAINER_NAME),
            ("network", NETWORK_NAME),
            ("volume", VOLUME_NAME),
        ] {
            if self.docker_exists(kind, name)? {
                return Err(fixture_failure("fixture resource name collision"));
            }
        }
        if self
            .docker(&["image", "inspect", IMAGE, "--format", "{{.Id}}"])
            .is_err()
        {
            self.docker(&["pull", IMAGE])?;
        }
        Ok(())
    }

    fn wait_ready(&self) -> Result<(), PocFailure> {
        let deadline = Instant::now() + Duration::from_secs(self.config.startup_timeout_seconds);
        loop {
            let status = self.docker(&[
                "inspect",
                CONTAINER_NAME,
                "--format",
                "{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}",
            ])?;
            match status.as_str() {
                "healthy" => return Ok(()),
                "unhealthy" | "exited" | "dead" => {
                    return Err(fixture_failure("Oracle container failed readiness"));
                }
                _ if Instant::now() >= deadline => {
                    return Err(fixture_failure("Oracle container readiness timed out"));
                }
                _ => thread::sleep(Duration::from_secs(5)),
            }
        }
    }

    fn discover_port(&self) -> Result<u16, PocFailure> {
        let binding = self.docker(&["port", CONTAINER_NAME, "1521/tcp"])?;
        let (host, port) = binding
            .rsplit_once(':')
            .ok_or_else(|| fixture_failure("Oracle listener binding is malformed"))?;
        if host != "127.0.0.1" {
            return Err(fixture_failure("Oracle listener is not loopback-only"));
        }
        port.parse()
            .map_err(|_| fixture_failure("Oracle listener port is malformed"))
    }

    fn verify_network_ownership(&self) -> Result<(), PocFailure> {
        let attachments = self.docker(&[
            "network",
            "inspect",
            NETWORK_NAME,
            "--format",
            "{{len .Containers}}",
        ])?;
        if attachments != "1" {
            return Err(fixture_failure(
                "fixture network has unexpected attachments",
            ));
        }
        let label = self.docker(&[
            "network",
            "inspect",
            NETWORK_NAME,
            "--format",
            "{{index .Labels \"kuberic.oracle.owner\"}}",
        ])?;
        if label != self.journal.token {
            return Err(fixture_failure("fixture network ownership changed"));
        }
        Ok(())
    }

    fn provision_observer(&self) -> Result<(), PocFailure> {
        let credentials = self.credentials.as_ref().unwrap();
        self.execute_sql(&format!(
            "ALTER SESSION SET CONTAINER=FREEPDB1;\n\
             CREATE USER {} IDENTIFIED BY \"{}\";\n\
             GRANT CREATE SESSION TO {};\n\
             GRANT SELECT ON SYS.V_$DATABASE TO {};\n\
             GRANT SELECT ON SYS.V_$INSTANCE TO {};",
            credentials.username,
            credentials.password.as_str(),
            credentials.username,
            credentials.username,
            credentials.username,
        ))
    }

    fn execute_sql(&self, statements: &str) -> Result<(), PocFailure> {
        let script = format!(
            "WHENEVER SQLERROR EXIT SQL.SQLCODE\nSET ECHO OFF FEEDBACK OFF HEADING OFF\n{statements}\nEXIT\n"
        );
        let output = self.docker_with_input(
            &[
                "exec",
                "-i",
                CONTAINER_NAME,
                "bash",
                "-lc",
                "sqlplus -s '/ as sysdba'",
            ],
            script.as_bytes(),
        )?;
        if !output.trim().is_empty() {
            return Err(fixture_failure(
                "Oracle fixture SQL returned unexpected output",
            ));
        }
        Ok(())
    }

    fn query_scalar(&self, prefix: &str, query: &str) -> Result<String, PocFailure> {
        let script = format!(
            "WHENEVER SQLERROR EXIT SQL.SQLCODE\n\
             SET ECHO OFF FEEDBACK OFF HEADING OFF PAGESIZE 0 VERIFY OFF\n\
             {prefix}\nPROMPT KUBERIC_RESULT_BEGIN\n{query};\nPROMPT KUBERIC_RESULT_END\nEXIT\n"
        );
        let output = self.docker_with_input(
            &[
                "exec",
                "-i",
                CONTAINER_NAME,
                "bash",
                "-lc",
                "sqlplus -s '/ as sysdba'",
            ],
            script.as_bytes(),
        )?;
        let result = output
            .split_once("KUBERIC_RESULT_BEGIN")
            .and_then(|(_, rest)| rest.split_once("KUBERIC_RESULT_END"))
            .map(|(value, _)| value.trim())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| fixture_failure("Oracle fixture query returned no result"))?;
        Ok(result.to_owned())
    }

    fn persist_journal(&mut self) -> Result<(), PocFailure> {
        self.journal_identity = Some(self.journal.persist(&self.config.root)?);
        Ok(())
    }

    fn cleanup_inner(&mut self) -> Result<(), PocFailure> {
        let mut failure: Option<PocFailure> = None;
        let trace_path = self.connection_trace_path();
        if trace_path.exists()
            && let Err(trace_failure) = self.remove_connection_trace(&trace_path)
        {
            failure = Some(trace_failure);
        }
        if !self.journal.container_id.is_empty() {
            match self.verify_container_identity() {
                Ok(()) => {
                    if self.docker(&["rm", "-f", CONTAINER_NAME]).is_err() {
                        failure = Some(cleanup_failure("owned container could not be removed"));
                    }
                }
                Err(identity) => failure = Some(identity),
            }
        }
        if !self.journal.network_id.is_empty() {
            match self.verify_network_identity() {
                Ok(()) => {
                    if self.docker(&["network", "rm", NETWORK_NAME]).is_err() && failure.is_none() {
                        failure = Some(cleanup_failure("owned network could not be removed"));
                    }
                }
                Err(identity) if failure.is_none() => failure = Some(identity),
                Err(_) => {}
            }
        }
        if !self.journal.volume_driver.is_empty() {
            match self.verify_volume_identity() {
                Ok(()) => {
                    if self.docker(&["volume", "rm", VOLUME_NAME]).is_err() && failure.is_none() {
                        failure = Some(cleanup_failure("owned volume could not be removed"));
                    }
                }
                Err(identity) if failure.is_none() => failure = Some(identity),
                Err(_) => {}
            }
        }

        for owned in [
            self.journal.username_file.as_ref(),
            self.journal.password_file.as_ref(),
            self.journal.admin_env_file.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if owned.1.matches_path(&owned.0) {
                if std::fs::remove_file(&owned.0).is_err() && failure.is_none() {
                    failure = Some(cleanup_failure(
                        "owned credential file could not be removed",
                    ));
                }
            } else if failure.is_none() {
                failure = Some(cleanup_failure("owned credential file identity changed"));
            }
        }
        self.credentials = None;

        let journal_path = self.config.root.join("journal.json");
        if let Some(identity) = self.journal_identity.take() {
            if identity.matches_path(&journal_path) {
                if std::fs::remove_file(&journal_path).is_err() && failure.is_none() {
                    failure = Some(cleanup_failure("fixture journal could not be removed"));
                }
            } else if failure.is_none() {
                failure = Some(cleanup_failure("fixture journal identity changed"));
            }
        }
        if failure.is_none()
            && let Some(owner) = self.owner.take()
            && let Err(owner_failure) = owner.release()
        {
            failure = Some(owner_failure);
        }
        match failure {
            Some(failure) => Err(failure),
            None => Ok(()),
        }
    }

    fn verify_container_identity(&self) -> Result<(), PocFailure> {
        let current = self.docker(&["inspect", CONTAINER_NAME, "--format", "{{.Id}}"])?;
        if current != self.journal.container_id {
            return Err(cleanup_failure("owned container identity changed"));
        }
        let label = self.docker(&[
            "inspect",
            CONTAINER_NAME,
            "--format",
            "{{index .Config.Labels \"kuberic.oracle.owner\"}}",
        ])?;
        if label != self.journal.token {
            return Err(cleanup_failure("owned container ownership changed"));
        }
        Ok(())
    }

    fn verify_network_identity(&self) -> Result<(), PocFailure> {
        let current = self.docker(&["network", "inspect", NETWORK_NAME, "--format", "{{.Id}}"])?;
        if current != self.journal.network_id {
            return Err(cleanup_failure("owned network identity changed"));
        }
        Ok(())
    }

    fn verify_volume_identity(&self) -> Result<(), PocFailure> {
        let current =
            self.docker(&["volume", "inspect", VOLUME_NAME, "--format", "{{.Driver}}"])?;
        if current != self.journal.volume_driver {
            return Err(cleanup_failure("owned volume identity changed"));
        }
        let label = self.docker(&[
            "volume",
            "inspect",
            VOLUME_NAME,
            "--format",
            "{{index .Labels \"kuberic.oracle.owner\"}}",
        ])?;
        if label != self.journal.token {
            return Err(cleanup_failure("owned volume ownership changed"));
        }
        Ok(())
    }

    fn docker_exists(&self, kind: &str, name: &str) -> Result<bool, PocFailure> {
        let status = Command::new("docker")
            .arg("-H")
            .arg(&self.config.docker_host)
            .arg(kind)
            .arg("inspect")
            .arg(name)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| fixture_failure("Docker command could not be started"))?;
        Ok(status.success())
    }

    fn docker(&self, args: &[&str]) -> Result<String, PocFailure> {
        let output = Command::new("docker")
            .arg("-H")
            .arg(&self.config.docker_host)
            .args(args)
            .output()
            .map_err(|_| fixture_failure("Docker command could not be started"))?;
        if !output.status.success() {
            return Err(fixture_failure("Docker fixture operation failed"));
        }
        String::from_utf8(output.stdout)
            .map(|value| value.trim().to_owned())
            .map_err(|_| fixture_failure("Docker command output was invalid"))
    }

    fn docker_with_input(&self, args: &[&str], input: &[u8]) -> Result<String, PocFailure> {
        let mut child = Command::new("docker")
            .arg("-H")
            .arg(&self.config.docker_host)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| fixture_failure("Docker command could not be started"))?;
        child
            .stdin
            .take()
            .ok_or_else(|| fixture_failure("Docker input pipe is unavailable"))?
            .write_all(input)
            .map_err(|_| fixture_failure("Docker input pipe failed"))?;
        let output = child
            .wait_with_output()
            .map_err(|_| fixture_failure("Docker command could not be collected"))?;
        if !output.status.success() {
            return Err(fixture_failure("Oracle fixture SQL failed"));
        }
        String::from_utf8(output.stdout)
            .map_err(|_| fixture_failure("Oracle fixture SQL output was invalid"))
    }
}

fn fixture_failure(message: &'static str) -> PocFailure {
    PocFailure::new(PocStage::Fixture, PocCategory::Fixture, message)
}

fn cleanup_failure(message: &'static str) -> PocFailure {
    PocFailure::new(PocStage::Cleanup, PocCategory::Cleanup, message)
}
