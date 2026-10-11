use std::{
    cell::Cell,
    ffi::CString,
    fs::OpenOptions,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use kuberic_oracle::{DatabaseIncarnationIdentity, DecimalScn};

use crate::poc::{PocCategory, PocEndpoint, PocFailure, PocStage};
use zeroize::Zeroizing;

use super::{
    config::{FixtureConfig, IMAGE},
    credentials::FixtureCredentials,
    owner::{FileIdentity, OwnerClaim, OwnershipJournal},
};

const NETWORK_NAME: &str = "kuberic-oracle-poc-network";
const VOLUME_NAME: &str = "kuberic-oracle-poc-volume";
const CONTAINER_NAME: &str = "kuberic-oracle-poc-database";
const OBSERVER: &str = "KUBERIC_OBSERVER";
const MAX_DOCKER_OUTPUT_BYTES: usize = 1024 * 1024;
const DOCKER_OPERATION_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureResourceEvidence {
    pub container_id: String,
    pub container_owner_label: String,
    pub volume_identity: String,
    pub volume_name: String,
    pub volume_owner_label: String,
    pub network_id: String,
    pub network_owner_label: String,
    pub attached_network_id: String,
    pub image: String,
    pub host_port: u16,
}

pub struct OracleFixture {
    config: FixtureConfig,
    owner: Option<OwnerClaim>,
    journal: OwnershipJournal,
    journal_identity: Option<FileIdentity>,
    credentials: Option<FixtureCredentials>,
    database_id: u64,
    database_unique_name: String,
    database_incarnation: Option<DatabaseIncarnationIdentity>,
    trace_identity: Cell<Option<FileIdentity>>,
    operation_deadline: Cell<Option<Instant>>,
    cleanup_attempted: bool,
}

impl OracleFixture {
    pub fn start(config: FixtureConfig) -> Result<Self, PocFailure> {
        let timeout = Duration::from_secs(config.startup_timeout_seconds);
        Self::start_until(config, Instant::now() + timeout)
    }

    pub fn start_until(
        config: FixtureConfig,
        operation_deadline: Instant,
    ) -> Result<Self, PocFailure> {
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
            database_incarnation: None,
            trace_identity: Cell::new(None),
            operation_deadline: Cell::new(Some(operation_deadline)),
            cleanup_attempted: false,
        };
        let operation = fixture.start_inner(operation_deadline);
        fixture.operation_deadline.set(None);
        if let Err(operation) = operation {
            fixture.cleanup_attempted = true;
            let cleanup = fixture.cleanup_inner().err();
            return Err(match cleanup {
                Some(cleanup) => operation.with_cleanup(cleanup),
                None => operation,
            });
        }
        Ok(fixture)
    }

    pub fn cleanup_existing(config: FixtureConfig) -> Result<(), PocFailure> {
        let (journal, journal_identity) = OwnershipJournal::load(&config.root)?;
        let owner = OwnerClaim::recover(&config.root, &journal.token)?;
        let mut fixture = Self {
            config,
            owner: Some(owner),
            journal,
            journal_identity: Some(journal_identity),
            credentials: None,
            database_id: 0,
            database_unique_name: String::new(),
            database_incarnation: None,
            trace_identity: Cell::new(None),
            operation_deadline: Cell::new(None),
            cleanup_attempted: true,
        };
        fixture.cleanup_inner()
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

    pub fn database_incarnation(&self) -> DatabaseIncarnationIdentity {
        self.database_incarnation.unwrap()
    }

    pub fn resource_evidence(&mut self) -> Result<FixtureResourceEvidence, PocFailure> {
        let container_id = self.verify_container_identity()?;
        let network_id = self.verify_network_identity()?;
        let volume_identity = self.verify_volume_identity()?;
        let (volume_name, attached_network_id) =
            self.verify_container_mount_and_network(&network_id)?;
        let container_owner_label = self.inspect_container_owner_label()?;
        let network_owner_label = self.inspect_network_owner_label()?;
        let volume_owner_label = self.inspect_volume_owner_label()?;
        let host_port = self.discover_port()?;
        if self.journal.host_port != host_port {
            self.journal.host_port = host_port;
            self.persist_journal()?;
        }
        Ok(FixtureResourceEvidence {
            container_id,
            container_owner_label,
            volume_identity,
            volume_name,
            volume_owner_label,
            network_id,
            network_owner_label,
            attached_network_id,
            image: self.journal.image.clone(),
            host_port,
        })
    }

    pub fn restart_container(
        &mut self,
        operation_deadline: Instant,
    ) -> Result<FixtureResourceEvidence, PocFailure> {
        self.operation_deadline.set(Some(operation_deadline));
        let result = self.restart_container_inner(operation_deadline);
        self.operation_deadline.set(None);
        result
    }

    fn restart_container_inner(
        &mut self,
        operation_deadline: Instant,
    ) -> Result<FixtureResourceEvidence, PocFailure> {
        let before = self.resource_evidence()?;
        self.docker_with_deadline(
            &["restart", &before.container_id],
            operation_deadline,
            DOCKER_OPERATION_TIMEOUT,
        )?;
        self.wait_ready(operation_deadline)?;
        self.journal.host_port = self.discover_port()?;
        self.persist_journal()?;
        self.verify_privileges()?;
        self.refresh_database_identity()?;
        self.resource_evidence()
    }

    pub fn recreate_container_same_volume(
        &mut self,
        operation_deadline: Instant,
    ) -> Result<FixtureResourceEvidence, PocFailure> {
        self.operation_deadline.set(Some(operation_deadline));
        let result = self.recreate_container_same_volume_inner(operation_deadline);
        self.operation_deadline.set(None);
        result
    }

    fn recreate_container_same_volume_inner(
        &mut self,
        operation_deadline: Instant,
    ) -> Result<FixtureResourceEvidence, PocFailure> {
        let before = self.resource_evidence()?;
        self.docker_with_deadline(
            &["rm", "-f", &before.container_id],
            operation_deadline,
            DOCKER_OPERATION_TIMEOUT,
        )?;
        if self.docker_exists("container", CONTAINER_NAME)? {
            return Err(fixture_failure("owned container removal was not confirmed"));
        }
        self.journal.container_id.clear();
        self.journal.host_port = 0;
        self.persist_journal()?;
        self.verify_network_identity()?;
        self.verify_volume_identity()?;
        self.create_container(operation_deadline)?;
        self.wait_ready(operation_deadline)?;
        self.journal.host_port = self.discover_port()?;
        self.persist_journal()?;
        self.verify_network_ownership()?;
        self.verify_privileges()?;
        self.refresh_database_identity()?;
        self.resource_evidence()
    }

    pub fn replace_with_fresh_volume(
        &mut self,
        operation_deadline: Instant,
    ) -> Result<FixtureResourceEvidence, PocFailure> {
        self.operation_deadline.set(Some(operation_deadline));
        let result = self.replace_with_fresh_volume_inner(operation_deadline);
        self.operation_deadline.set(None);
        result
    }

    fn replace_with_fresh_volume_inner(
        &mut self,
        operation_deadline: Instant,
    ) -> Result<FixtureResourceEvidence, PocFailure> {
        let before = self.resource_evidence()?;
        self.docker_with_deadline(
            &["rm", "-f", &before.container_id],
            operation_deadline,
            DOCKER_OPERATION_TIMEOUT,
        )?;
        if self.docker_exists("container", CONTAINER_NAME)? {
            return Err(fixture_failure("owned container removal was not confirmed"));
        }
        self.journal.container_id.clear();
        self.journal.host_port = 0;
        self.persist_journal()?;

        self.verify_volume_identity()?;
        self.docker_with_deadline(
            &["volume", "rm", VOLUME_NAME],
            operation_deadline,
            DOCKER_OPERATION_TIMEOUT,
        )?;
        if self.docker_exists("volume", VOLUME_NAME)? {
            return Err(fixture_failure("owned volume removal was not confirmed"));
        }
        self.journal.volume_identity.clear();
        self.persist_journal()?;

        self.verify_network_identity()?;
        self.create_volume(operation_deadline)?;
        self.create_container(operation_deadline)?;
        self.wait_ready(operation_deadline)?;
        self.journal.host_port = self.discover_port()?;
        self.persist_journal()?;
        self.verify_network_ownership()?;
        self.provision_observer()?;
        self.verify_privileges()?;
        self.refresh_database_identity()?;
        self.resource_evidence()
    }

    pub fn create_wrong_password_file(&mut self) -> Result<(PathBuf, FileIdentity), PocFailure> {
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
        let identity = FileIdentity::from_path(&path)?;
        self.journal.temporary_files.push((path.clone(), identity));
        if let Err(error) = self.persist_journal() {
            let _ = std::fs::remove_file(&path);
            self.journal
                .temporary_files
                .retain(|(owned, _)| owned != &path);
            return Err(error);
        }
        Ok((path, identity))
    }

    pub fn connection_trace_path(&self) -> PathBuf {
        self.config.root.join("worker-connect.trace")
    }

    pub fn verify_loopback_trace(&mut self, path: &Path) -> Result<FileIdentity, PocFailure> {
        let identity = FileIdentity::from_path(path)?;
        self.trace_identity.set(Some(identity));
        if !self
            .journal
            .temporary_files
            .iter()
            .any(|(owned, _)| owned == path)
        {
            self.journal
                .temporary_files
                .push((path.to_path_buf(), identity));
            self.persist_journal()?;
        }
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
        if !identity.matches_path(path) {
            return Err(fixture_failure(
                "worker connection trace changed during verification",
            ));
        }
        Ok(identity)
    }

    pub fn remove_connection_trace(&mut self, path: &Path) -> Result<(), PocFailure> {
        if !path.starts_with(&self.config.root) {
            return Err(cleanup_failure(
                "worker trace path escaped the fixture root",
            ));
        }
        match self.trace_identity.get() {
            Some(identity) => {
                self.remove_owned_file(path, identity)?;
                self.trace_identity.set(None);
                Ok(())
            }
            None if !path.exists() => Ok(()),
            None => Err(cleanup_failure("worker trace identity was not recorded")),
        }
    }

    pub fn remove_owned_file(
        &mut self,
        path: &Path,
        identity: FileIdentity,
    ) -> Result<(), PocFailure> {
        if !identity.matches_path(path) {
            return Err(cleanup_failure("owned temporary file identity changed"));
        }
        std::fs::remove_file(path)
            .map_err(|_| cleanup_failure("owned temporary file could not be removed"))?;
        self.journal
            .temporary_files
            .retain(|(owned, _)| owned != path);
        self.persist_journal()
    }

    pub fn revoke_view(&self, view: &str) -> Result<(), PocFailure> {
        if !matches!(
            view,
            "SYS.V_$DATABASE" | "SYS.V_$INSTANCE" | "SYS.V_$DATABASE_INCARNATION"
        ) {
            return Err(fixture_failure("unsupported fixture grant mutation"));
        }
        self.execute_sql(&format!(
            "ALTER SESSION SET CONTAINER=FREEPDB1;\nREVOKE SELECT ON {view} FROM {OBSERVER};"
        ))
    }

    pub fn grant_view(&self, view: &str) -> Result<(), PocFailure> {
        if !matches!(
            view,
            "SYS.V_$DATABASE" | "SYS.V_$INSTANCE" | "SYS.V_$DATABASE_INCARNATION"
        ) {
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
                 (SELECT COUNT(*) FROM DBA_SYS_PRIVS WHERE GRANTEE='{OBSERVER}') || ':' || \
                 (SELECT COUNT(*) FROM DBA_SYS_PRIVS WHERE GRANTEE='{OBSERVER}' AND PRIVILEGE='CREATE SESSION' AND ADMIN_OPTION='NO') || ':' || \
                 (SELECT COUNT(*) FROM DBA_SYS_PRIVS WHERE GRANTEE='{OBSERVER}' AND PRIVILEGE <> 'CREATE SESSION') || ':' || \
                 (SELECT COUNT(*) FROM DBA_TAB_PRIVS WHERE GRANTEE='{OBSERVER}') || ':' || \
                 (SELECT COUNT(*) FROM DBA_TAB_PRIVS WHERE GRANTEE='{OBSERVER}' AND PRIVILEGE='SELECT' AND OWNER='SYS' AND TABLE_NAME IN ('V_$DATABASE','V_$INSTANCE','V_$DATABASE_INCARNATION') AND GRANTABLE='NO') || ':' || \
                 (SELECT COUNT(*) FROM DBA_TAB_PRIVS WHERE GRANTEE='{OBSERVER}' AND NOT (PRIVILEGE='SELECT' AND OWNER='SYS' AND TABLE_NAME IN ('V_$DATABASE','V_$INSTANCE','V_$DATABASE_INCARNATION') AND GRANTABLE='NO')) || ':' || \
                 (SELECT COUNT(*) FROM V$PWFILE_USERS WHERE USERNAME='{OBSERVER}') || ':' || \
                 (SELECT COUNT(*) FROM DBA_TAB_PRIVS WHERE GRANTEE='PUBLIC' AND OWNER='SYS' AND TABLE_NAME='PRODUCT_COMPONENT_VERSION' AND PRIVILEGE='READ') || ':' || \
                 (SELECT COUNT(*) FROM DBA_SYS_PRIVS WHERE GRANTEE='PUBLIC' AND PRIVILEGE IN ('SELECT ANY TABLE','SELECT ANY DICTIONARY','CREATE USER','ALTER USER','DROP USER','ALTER DATABASE','ALTER SYSTEM')) || ':' || \
                 (SELECT COUNT(*) FROM DBA_ROLE_PRIVS WHERE GRANTEE='PUBLIC') || ':' || \
                 (SELECT COUNT(*) FROM DBA_COL_PRIVS WHERE GRANTEE='{OBSERVER}') \
                 FROM DUAL"
            ),
        )?;
        if result != "0:1:1:0:3:3:0:0:1:0:0:0" {
            return Err(fixture_failure(
                "observer effective privileges exceed the PoC profile",
            ));
        }
        Ok(())
    }

    pub fn cleanup(mut self) -> Result<(), PocFailure> {
        self.cleanup_attempted = true;
        self.cleanup_inner()
    }

    fn start_inner(&mut self, operation_deadline: Instant) -> Result<(), PocFailure> {
        self.preflight(operation_deadline)?;
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

        self.journal.network_expected = true;
        self.persist_journal()?;
        self.journal.network_id = self.docker(&[
            "network",
            "create",
            "--label",
            &format!("kuberic.oracle.owner={}", self.journal.token),
            NETWORK_NAME,
        ])?;
        self.persist_journal()?;

        self.create_volume(operation_deadline)?;
        self.create_container(operation_deadline)?;
        self.wait_ready(operation_deadline)?;
        self.journal.host_port = self.discover_port()?;
        self.persist_journal()?;
        self.verify_network_ownership()?;
        self.provision_observer()?;
        self.verify_privileges()?;
        self.refresh_database_identity()
    }

    fn refresh_database_identity(&mut self) -> Result<(), PocFailure> {
        let identity = self.query_scalar(
            "ALTER SESSION SET CONTAINER=FREEPDB1;",
            "SELECT d.DBID || ':' || d.DB_UNIQUE_NAME || ':' || \
                    i.INCARNATION# || ':' || i.RESETLOGS_ID || ':' || \
                    i.RESETLOGS_CHANGE# \
             FROM V$DATABASE d \
             CROSS JOIN V$DATABASE_INCARNATION i \
             WHERE i.STATUS='CURRENT' AND i.CON_ID=0",
        )?;
        let mut identity = identity.split(':');
        let database_id = identity
            .next()
            .ok_or_else(|| fixture_failure("database identity query was malformed"))?;
        let unique_name = identity
            .next()
            .ok_or_else(|| fixture_failure("database identity query was malformed"))?;
        let incarnation_number = identity
            .next()
            .ok_or_else(|| fixture_failure("database identity query was malformed"))?;
        let resetlogs_id = identity
            .next()
            .ok_or_else(|| fixture_failure("database identity query was malformed"))?;
        let resetlogs_change = identity
            .next()
            .ok_or_else(|| fixture_failure("database identity query was malformed"))?;
        if identity.next().is_some() {
            return Err(fixture_failure("database identity query was malformed"));
        }
        self.database_id = database_id
            .parse()
            .map_err(|_| fixture_failure("database ID query was malformed"))?;
        self.database_unique_name = unique_name.to_owned();
        self.database_incarnation = Some(DatabaseIncarnationIdentity {
            incarnation_number: incarnation_number
                .parse()
                .map_err(|_| fixture_failure("database incarnation query was malformed"))?,
            resetlogs_id: resetlogs_id
                .parse()
                .map_err(|_| fixture_failure("database incarnation query was malformed"))?,
            resetlogs_change: DecimalScn::parse(resetlogs_change)
                .map_err(|_| fixture_failure("database incarnation query was malformed"))?,
        });
        Ok(())
    }

    fn preflight(&self, operation_deadline: Instant) -> Result<(), PocFailure> {
        if std::env::consts::ARCH != "x86_64" || std::env::consts::OS != "linux" {
            return Err(fixture_failure("live PoC requires Linux x86-64"));
        }
        let architecture = self.docker(&["info", "--format", "{{.Architecture}}"])?;
        if architecture != "x86_64" {
            return Err(fixture_failure("Docker architecture is unsupported"));
        }
        let strace = Command::new("strace")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| fixture_failure("strace is required for the live PoC"))?;
        if !strace.success() {
            return Err(fixture_failure("strace is required for the live PoC"));
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
            let docker_root = self.docker(&["info", "--format", "{{.DockerRootDir}}"])?;
            if available_bytes(Path::new(&docker_root))? < 12 * 1024 * 1024 * 1024 {
                return Err(fixture_failure(
                    "Docker storage is insufficient for the Oracle image",
                ));
            }
            self.docker_with_deadline(
                &["pull", IMAGE],
                operation_deadline,
                Duration::from_secs(self.config.startup_timeout_seconds),
            )?;
        }
        Ok(())
    }

    fn create_volume(&mut self, operation_deadline: Instant) -> Result<(), PocFailure> {
        self.journal.volume_expected = true;
        self.persist_journal()?;
        self.docker_with_deadline(
            &[
                "volume",
                "create",
                "--label",
                &format!("kuberic.oracle.owner={}", self.journal.token),
                VOLUME_NAME,
            ],
            operation_deadline,
            DOCKER_OPERATION_TIMEOUT,
        )?;
        self.journal.volume_identity = self.docker_with_deadline(
            &[
                "volume",
                "inspect",
                VOLUME_NAME,
                "--format",
                "{{.CreatedAt}}|{{.Mountpoint}}|{{.Driver}}",
            ],
            operation_deadline,
            DOCKER_OPERATION_TIMEOUT,
        )?;
        self.persist_journal()
    }

    fn create_container(&mut self, operation_deadline: Instant) -> Result<(), PocFailure> {
        let admin_env_file = self
            .credentials
            .as_ref()
            .unwrap()
            .admin_env_file
            .to_string_lossy()
            .into_owned();
        self.journal.container_expected = true;
        self.persist_journal()?;
        self.journal.container_id = self.docker_with_deadline(
            &[
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
                &admin_env_file,
                IMAGE,
            ],
            operation_deadline,
            Duration::from_secs(self.config.startup_timeout_seconds),
        )?;
        self.persist_journal()
    }

    fn wait_ready(&self, operation_deadline: Instant) -> Result<(), PocFailure> {
        let startup_deadline =
            Instant::now() + Duration::from_secs(self.config.startup_timeout_seconds);
        let deadline = operation_deadline.min(startup_deadline);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(timeout_failure());
            }
            let status = self.docker_with_timeout(
                &[
                    "inspect",
                    CONTAINER_NAME,
                    "--format",
                    "{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}",
                ],
                remaining.min(Duration::from_secs(30)),
            )?;
            match status.as_str() {
                "healthy" => return Ok(()),
                "unhealthy" | "exited" | "dead" => {
                    return Err(fixture_failure("Oracle container failed readiness"));
                }
                _ if Instant::now() >= deadline => {
                    return Err(timeout_failure());
                }
                _ => thread::sleep(remaining.min(Duration::from_secs(5))),
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

    fn verify_container_mount_and_network(
        &self,
        network_id: &str,
    ) -> Result<(String, String), PocFailure> {
        let mounted_volume = self.docker(&[
            "inspect",
            CONTAINER_NAME,
            "--format",
            "{{range .Mounts}}{{if eq .Destination \"/opt/oracle/oradata\"}}{{.Name}}{{end}}{{end}}",
        ])?;
        if mounted_volume != VOLUME_NAME {
            return Err(cleanup_failure("owned container volume attachment changed"));
        }
        let attached_network = self.docker(&[
            "inspect",
            CONTAINER_NAME,
            "--format",
            "{{(index .NetworkSettings.Networks \"kuberic-oracle-poc-network\").NetworkID}}",
        ])?;
        if attached_network != network_id {
            return Err(cleanup_failure(
                "owned container network attachment changed",
            ));
        }
        Ok((mounted_volume, attached_network))
    }

    fn provision_observer(&self) -> Result<(), PocFailure> {
        let credentials = self.credentials.as_ref().unwrap();
        let statements = Zeroizing::new(format!(
            "ALTER SESSION SET CONTAINER=FREEPDB1;\n\
             CREATE USER {} IDENTIFIED BY \"{}\";\n\
             GRANT CREATE SESSION TO {};\n\
             GRANT SELECT ON SYS.V_$DATABASE TO {};\n\
             GRANT SELECT ON SYS.V_$INSTANCE TO {};\n\
             GRANT SELECT ON SYS.V_$DATABASE_INCARNATION TO {};",
            credentials.username,
            credentials.password.as_str(),
            credentials.username,
            credentials.username,
            credentials.username,
            credentials.username,
        ));
        self.execute_sql(&statements)
    }

    fn execute_sql(&self, statements: &str) -> Result<(), PocFailure> {
        let script = Zeroizing::new(format!(
            "WHENEVER SQLERROR EXIT SQL.SQLCODE\nSET ECHO OFF FEEDBACK OFF HEADING OFF\n{statements}\nEXIT\n"
        ));
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
        if trace_path.exists() {
            if let Err(trace_failure) = self.remove_connection_trace(&trace_path) {
                failure = Some(trace_failure);
            }
        } else {
            self.trace_identity.set(None);
        }
        if self.journal.container_expected {
            match self.docker_exists("container", CONTAINER_NAME) {
                Ok(true) => match self.verify_container_identity() {
                    Ok(container_id) => {
                        let attachment_check =
                            if self.journal.network_expected && self.journal.volume_expected {
                                self.verify_network_identity().and_then(|network_id| {
                                    self.verify_volume_identity()?;
                                    self.verify_container_mount_and_network(&network_id)
                                        .map(|_| ())
                                })
                            } else {
                                Ok(())
                            };
                        if let Err(identity) = attachment_check {
                            failure = Some(identity);
                        } else if self.docker(&["rm", "-f", &container_id]).is_err() {
                            failure = Some(cleanup_failure("owned container could not be removed"));
                        }
                    }
                    Err(identity) => failure = Some(identity),
                },
                Ok(false) => {}
                Err(inspect) => failure = Some(inspect),
            }
        }
        if self.journal.network_expected {
            match self.docker_exists("network", NETWORK_NAME) {
                Ok(true) => match self.verify_network_identity() {
                    Ok(network_id) => {
                        if self.docker(&["network", "rm", &network_id]).is_err()
                            && failure.is_none()
                        {
                            failure = Some(cleanup_failure("owned network could not be removed"));
                        }
                    }
                    Err(identity) if failure.is_none() => failure = Some(identity),
                    Err(_) => {}
                },
                Ok(false) => {}
                Err(inspect) if failure.is_none() => failure = Some(inspect),
                Err(_) => {}
            }
        }
        if self.journal.volume_expected {
            match self.docker_exists("volume", VOLUME_NAME) {
                Ok(true) => match self.verify_volume_identity() {
                    Ok(_) => {
                        if self.docker(&["volume", "rm", VOLUME_NAME]).is_err() && failure.is_none()
                        {
                            failure = Some(cleanup_failure("owned volume could not be removed"));
                        }
                    }
                    Err(identity) if failure.is_none() => failure = Some(identity),
                    Err(_) => {}
                },
                Ok(false) => {}
                Err(inspect) if failure.is_none() => failure = Some(inspect),
                Err(_) => {}
            }
        }
        if let Some(failure) = failure {
            return Err(failure);
        }

        for (path, identity) in self.journal.temporary_files.clone() {
            if path.exists() {
                self.remove_owned_file(&path, identity)?;
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
            if !owned.0.exists() {
                continue;
            } else if owned.1.matches_path(&owned.0) {
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
        if let Some(failure) = failure {
            return Err(failure);
        }

        let journal_path = self.config.root.join("journal.json");
        if let Some(identity) = self.journal_identity {
            if identity.matches_path(&journal_path) {
                if std::fs::remove_file(&journal_path).is_err() {
                    failure = Some(cleanup_failure("fixture journal could not be removed"));
                } else {
                    self.journal_identity = None;
                }
            } else {
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

    fn verify_container_identity(&mut self) -> Result<String, PocFailure> {
        let current = self.docker(&["inspect", CONTAINER_NAME, "--format", "{{.Id}}"])?;
        if !self.journal.container_id.is_empty() && current != self.journal.container_id {
            return Err(cleanup_failure("owned container identity changed"));
        }
        let label = self.inspect_container_owner_label()?;
        if label != self.journal.token {
            return Err(cleanup_failure("owned container ownership changed"));
        }
        let image = self.docker(&["inspect", CONTAINER_NAME, "--format", "{{.Config.Image}}"])?;
        if image != self.journal.image {
            return Err(cleanup_failure("owned container image changed"));
        }
        if self.journal.container_id.is_empty() {
            self.journal.container_id = current.clone();
            self.persist_journal()?;
        }
        Ok(current)
    }

    fn verify_network_identity(&mut self) -> Result<String, PocFailure> {
        let current = self.docker(&["network", "inspect", NETWORK_NAME, "--format", "{{.Id}}"])?;
        if !self.journal.network_id.is_empty() && current != self.journal.network_id {
            return Err(cleanup_failure("owned network identity changed"));
        }
        let label = self.inspect_network_owner_label()?;
        if label != self.journal.token {
            return Err(cleanup_failure("owned network ownership changed"));
        }
        if self.journal.network_id.is_empty() {
            self.journal.network_id = current.clone();
            self.persist_journal()?;
        }
        Ok(current)
    }

    fn verify_volume_identity(&mut self) -> Result<String, PocFailure> {
        let current = self.docker(&[
            "volume",
            "inspect",
            VOLUME_NAME,
            "--format",
            "{{.CreatedAt}}|{{.Mountpoint}}|{{.Driver}}",
        ])?;
        if !self.journal.volume_identity.is_empty() && current != self.journal.volume_identity {
            return Err(cleanup_failure("owned volume identity changed"));
        }
        let label = self.inspect_volume_owner_label()?;
        if label != self.journal.token {
            return Err(cleanup_failure("owned volume ownership changed"));
        }
        if self.journal.volume_identity.is_empty() {
            self.journal.volume_identity = current.clone();
            self.persist_journal()?;
        }
        Ok(current)
    }

    fn inspect_container_owner_label(&self) -> Result<String, PocFailure> {
        self.docker(&[
            "inspect",
            CONTAINER_NAME,
            "--format",
            "{{index .Config.Labels \"kuberic.oracle.owner\"}}",
        ])
    }

    fn inspect_network_owner_label(&self) -> Result<String, PocFailure> {
        self.docker(&[
            "network",
            "inspect",
            NETWORK_NAME,
            "--format",
            "{{index .Labels \"kuberic.oracle.owner\"}}",
        ])
    }

    fn inspect_volume_owner_label(&self) -> Result<String, PocFailure> {
        self.docker(&[
            "volume",
            "inspect",
            VOLUME_NAME,
            "--format",
            "{{index .Labels \"kuberic.oracle.owner\"}}",
        ])
    }

    fn docker_exists(&self, kind: &str, name: &str) -> Result<bool, PocFailure> {
        let output = match kind {
            "container" => self.docker(&["ps", "-a", "--format", "{{.Names}}"])?,
            "network" => self.docker(&["network", "ls", "--format", "{{.Name}}"])?,
            "volume" => self.docker(&["volume", "ls", "--format", "{{.Name}}"])?,
            _ => return Err(fixture_failure("unsupported Docker resource type")),
        };
        Ok(output.lines().any(|value| value == name))
    }

    fn docker(&self, args: &[&str]) -> Result<String, PocFailure> {
        self.docker_with_timeout(args, DOCKER_OPERATION_TIMEOUT)
    }

    fn docker_with_timeout(&self, args: &[&str], timeout: Duration) -> Result<String, PocFailure> {
        let timeout = match self.operation_deadline.get() {
            Some(deadline) => deadline
                .checked_duration_since(Instant::now())
                .filter(|remaining| !remaining.is_zero())
                .ok_or_else(timeout_failure)?
                .min(timeout),
            None => timeout,
        };
        let result = self.run_docker(args, None, timeout);
        if result.is_err()
            && self
                .operation_deadline
                .get()
                .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(timeout_failure());
        }
        result
    }

    fn docker_with_deadline(
        &self,
        args: &[&str],
        operation_deadline: Instant,
        local_limit: Duration,
    ) -> Result<String, PocFailure> {
        let timeout = operation_deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(timeout_failure)?
            .min(local_limit);
        self.docker_with_timeout(args, timeout)
    }

    fn docker_with_input(&self, args: &[&str], input: &[u8]) -> Result<String, PocFailure> {
        if input.len() > 4096 {
            return Err(fixture_failure("Docker input exceeds the fixture bound"));
        }
        let timeout = match self.operation_deadline.get() {
            Some(deadline) => deadline
                .checked_duration_since(Instant::now())
                .filter(|remaining| !remaining.is_zero())
                .ok_or_else(timeout_failure)?
                .min(DOCKER_OPERATION_TIMEOUT),
            None => DOCKER_OPERATION_TIMEOUT,
        };
        let result = self.run_docker(args, Some(input), timeout);
        if result.is_err()
            && self
                .operation_deadline
                .get()
                .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(timeout_failure());
        }
        result
    }

    fn run_docker(
        &self,
        args: &[&str],
        input: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<String, PocFailure> {
        let mut child = Command::new(&self.config.docker_program)
            .arg("-H")
            .arg(&self.config.docker_host)
            .args(args)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| fixture_failure("Docker command could not be started"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| fixture_failure("Docker output pipe is unavailable"))?;
        let reader = thread::spawn(move || {
            let mut output = Vec::new();
            stdout
                .take(MAX_DOCKER_OUTPUT_BYTES as u64 + 1)
                .read_to_end(&mut output)
                .map(|_| output)
        });
        if let Some(input) = input {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| fixture_failure("Docker input pipe is unavailable"))?;
            if stdin.write_all(input).is_err() {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(fixture_failure("Docker input pipe failed"));
            }
        }
        let deadline = Instant::now() + timeout;
        let status = loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|_| fixture_failure("Docker command status is unavailable"))?
            {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(fixture_failure("Docker command timed out"));
            }
            thread::sleep(Duration::from_millis(20));
        };
        let output = reader
            .join()
            .map_err(|_| fixture_failure("Docker output reader failed"))?
            .map_err(|_| fixture_failure("Docker command output could not be read"))?;
        if output.len() > MAX_DOCKER_OUTPUT_BYTES {
            return Err(fixture_failure("Docker command output exceeded the bound"));
        }
        if !status.success() {
            return Err(fixture_failure("Docker fixture operation failed"));
        }
        String::from_utf8(output)
            .map(|value| value.trim().to_owned())
            .map_err(|_| fixture_failure("Docker command output was invalid"))
    }
}

impl Drop for OracleFixture {
    fn drop(&mut self) {
        if self.owner.is_some() && !self.cleanup_attempted {
            let _ = self.cleanup_inner();
        }
    }
}

fn fixture_failure(message: &'static str) -> PocFailure {
    PocFailure::new(PocStage::Fixture, PocCategory::Fixture, message)
}

fn timeout_failure() -> PocFailure {
    PocFailure::new(
        PocStage::Fixture,
        PocCategory::Timeout,
        "fixture operation timed out",
    )
}

fn cleanup_failure(message: &'static str) -> PocFailure {
    PocFailure::new(PocStage::Cleanup, PocCategory::Cleanup, message)
}

fn available_bytes(path: &Path) -> Result<u64, PocFailure> {
    let encoded = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| fixture_failure("Docker root path is invalid"))?;
    let mut statistics = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(encoded.as_ptr(), statistics.as_mut_ptr()) } != 0 {
        return Err(fixture_failure("Docker storage could not be inspected"));
    }
    let statistics = unsafe { statistics.assume_init() };
    Ok(statistics.f_bavail.saturating_mul(statistics.f_frsize))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn cleanup_preserves_journal_when_docker_is_unavailable() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("fixture");
        let docker = directory.path().join("docker-fail");
        write_script(&docker, "#!/bin/sh\nexit 1\n");
        let owner = OwnerClaim::acquire(&root).unwrap();
        let journal = OwnershipJournal {
            token: owner.token.clone(),
            container_name: CONTAINER_NAME.to_owned(),
            container_expected: true,
            image: IMAGE.to_owned(),
            ..OwnershipJournal::default()
        };
        let journal_identity = journal.persist(&root).unwrap();
        let mut fixture = test_fixture(root.clone(), docker, owner, journal, journal_identity);

        assert!(fixture.cleanup_inner().is_err());
        assert!(root.join("journal.json").exists());
        assert!(root.join("owner.lock").exists());
    }

    #[test]
    fn cleanup_removes_container_by_recorded_id() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("fixture");
        let marker = directory.path().join("removed");
        let docker = directory.path().join("docker-fake");
        write_script(
            &docker,
            &format!(
                "#!/bin/sh\n\
                 shift 2\n\
                 if [ \"$1\" = ps ]; then echo {CONTAINER_NAME}; exit 0; fi\n\
                 if [ \"$1\" = inspect ]; then\n\
                   case \"$*\" in\n\
                     *Config.Image*) echo '{IMAGE}' ;;\n\
                     *Config.Labels*) echo token ;;\n\
                     *) echo container-id ;;\n\
                   esac\n\
                   exit 0\n\
                 fi\n\
                 if [ \"$1\" = rm ]; then echo \"$3\" > '{}'; exit 0; fi\n\
                 exit 0\n",
                marker.display()
            ),
        );
        let owner = OwnerClaim::acquire(&root).unwrap();
        let journal = OwnershipJournal {
            token: "token".to_owned(),
            container_name: CONTAINER_NAME.to_owned(),
            container_expected: true,
            container_id: "container-id".to_owned(),
            image: IMAGE.to_owned(),
            ..OwnershipJournal::default()
        };
        let journal_identity = journal.persist(&root).unwrap();
        let mut fixture = test_fixture(root, docker, owner, journal, journal_identity);

        fixture.cleanup_inner().unwrap();

        assert_eq!(
            std::fs::read_to_string(marker).unwrap().trim(),
            "container-id"
        );
    }

    #[test]
    fn cleanup_existing_adopts_unrecorded_owned_container() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("fixture");
        let marker = directory.path().join("removed");
        let docker = directory.path().join("docker-fake");
        let owner = OwnerClaim::acquire(&root).unwrap();
        write_script(
            &docker,
            &format!(
                "#!/bin/sh\n\
                 shift 2\n\
                 if [ \"$1\" = ps ]; then echo {CONTAINER_NAME}; exit 0; fi\n\
                 if [ \"$1\" = inspect ]; then\n\
                   case \"$*\" in\n\
                     *Config.Image*) echo '{IMAGE}' ;;\n\
                     *Config.Labels*) echo '{}' ;;\n\
                     *) echo adopted-container-id ;;\n\
                   esac\n\
                   exit 0\n\
                 fi\n\
                 if [ \"$1\" = rm ]; then echo \"$3\" > '{}'; exit 0; fi\n\
                 exit 0\n",
                owner.token,
                marker.display()
            ),
        );
        let journal = OwnershipJournal {
            token: owner.token.clone(),
            container_name: CONTAINER_NAME.to_owned(),
            container_expected: true,
            image: IMAGE.to_owned(),
            ..OwnershipJournal::default()
        };
        journal.persist(&root).unwrap();
        drop(owner);

        OracleFixture::cleanup_existing(FixtureConfig {
            docker_program: docker,
            docker_host: "fake".to_owned(),
            root: root.clone(),
            startup_timeout_seconds: 1,
        })
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(marker).unwrap().trim(),
            "adopted-container-id"
        );
        assert!(!root.exists());
    }

    #[test]
    fn cleanup_existing_adopts_unrecorded_owned_volume() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("fixture");
        let marker = directory.path().join("removed");
        let docker = directory.path().join("docker-fake");
        let owner = OwnerClaim::acquire(&root).unwrap();
        write_script(
            &docker,
            &format!(
                "#!/bin/sh\n\
                 shift 2\n\
                 if [ \"$1\" = ps ]; then exit 0; fi\n\
                 if [ \"$1\" = network ] && [ \"$2\" = ls ]; then exit 0; fi\n\
                 if [ \"$1\" = volume ] && [ \"$2\" = ls ]; then echo {VOLUME_NAME}; exit 0; fi\n\
                 if [ \"$1\" = volume ] && [ \"$2\" = inspect ]; then\n\
                   case \"$*\" in\n\
                     *Labels*) echo '{}' ;;\n\
                     *) echo 'created|/mount|local' ;;\n\
                   esac\n\
                   exit 0\n\
                 fi\n\
                 if [ \"$1\" = volume ] && [ \"$2\" = rm ]; then echo \"$3\" > '{}'; exit 0; fi\n\
                 exit 0\n",
                owner.token,
                marker.display()
            ),
        );
        let journal = OwnershipJournal {
            token: owner.token.clone(),
            volume_name: VOLUME_NAME.to_owned(),
            volume_expected: true,
            ..OwnershipJournal::default()
        };
        journal.persist(&root).unwrap();
        drop(owner);

        OracleFixture::cleanup_existing(FixtureConfig {
            docker_program: docker,
            docker_host: "fake".to_owned(),
            root: root.clone(),
            startup_timeout_seconds: 1,
        })
        .unwrap();

        assert_eq!(std::fs::read_to_string(marker).unwrap().trim(), VOLUME_NAME);
        assert!(!root.exists());
    }

    #[test]
    fn cleanup_existing_retains_foreign_unrecorded_container() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("fixture");
        let docker = directory.path().join("docker-fake");
        write_script(
            &docker,
            &format!(
                "#!/bin/sh\n\
                 shift 2\n\
                 if [ \"$1\" = ps ]; then echo {CONTAINER_NAME}; exit 0; fi\n\
                 if [ \"$1\" = inspect ]; then\n\
                   case \"$*\" in\n\
                     *Config.Image*) echo '{IMAGE}' ;;\n\
                     *Config.Labels*) echo foreign ;;\n\
                     *) echo foreign-container-id ;;\n\
                   esac\n\
                   exit 0\n\
                 fi\n\
                 exit 0\n"
            ),
        );
        let owner = OwnerClaim::acquire(&root).unwrap();
        let journal = OwnershipJournal {
            token: owner.token.clone(),
            container_name: CONTAINER_NAME.to_owned(),
            container_expected: true,
            image: IMAGE.to_owned(),
            ..OwnershipJournal::default()
        };
        journal.persist(&root).unwrap();
        drop(owner);

        assert!(
            OracleFixture::cleanup_existing(FixtureConfig {
                docker_program: docker,
                docker_host: "fake".to_owned(),
                root: root.clone(),
                startup_timeout_seconds: 1,
            })
            .is_err()
        );
        assert!(root.join("journal.json").exists());
        assert!(root.join("owner.lock").exists());
    }

    #[test]
    fn expired_start_deadline_rolls_back_owner_claim() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("fixture");
        let docker = directory.path().join("docker-fake");
        write_script(&docker, "#!/bin/sh\nexit 0\n");

        let failure = match OracleFixture::start_until(
            FixtureConfig {
                docker_program: docker,
                docker_host: "fake".to_owned(),
                root: root.clone(),
                startup_timeout_seconds: 1,
            },
            Instant::now(),
        ) {
            Ok(_) => panic!("expired fixture deadline unexpectedly succeeded"),
            Err(failure) => failure,
        };

        assert_eq!(failure.category(), PocCategory::Timeout);
        assert!(!root.exists());
    }

    #[test]
    fn expired_lifecycle_deadlines_fail_before_docker_mutation() {
        for operation in ["restart", "recreate", "replace"] {
            let directory = tempdir().unwrap();
            let root = directory.path().join("fixture");
            let docker = directory.path().join("docker-fake");
            write_script(&docker, "#!/bin/sh\nexit 0\n");
            let owner = OwnerClaim::acquire(&root).unwrap();
            let journal = OwnershipJournal {
                token: "token".to_owned(),
                container_name: CONTAINER_NAME.to_owned(),
                container_expected: true,
                container_id: "container-id".to_owned(),
                image: IMAGE.to_owned(),
                ..OwnershipJournal::default()
            };
            let journal_identity = journal.persist(&root).unwrap();
            let mut fixture = test_fixture(root, docker, owner, journal, journal_identity);

            let result = match operation {
                "restart" => fixture.restart_container(Instant::now()),
                "recreate" => fixture.recreate_container_same_volume(Instant::now()),
                "replace" => fixture.replace_with_fresh_volume(Instant::now()),
                _ => unreachable!(),
            };

            assert_eq!(result.unwrap_err().category(), PocCategory::Timeout);
        }
    }

    #[test]
    fn cleanup_existing_accepts_interruption_before_or_after_identity_clear() {
        for (container_id, volume_identity) in
            [("old-container", "old-volume|/mount|local"), ("", "")]
        {
            let directory = tempdir().unwrap();
            let root = directory.path().join("fixture");
            let docker = directory.path().join("docker-fake");
            write_script(&docker, "#!/bin/sh\nshift 2\nexit 0\n");
            let owner = OwnerClaim::acquire(&root).unwrap();
            let journal = OwnershipJournal {
                token: owner.token.clone(),
                container_name: CONTAINER_NAME.to_owned(),
                container_expected: true,
                container_id: container_id.to_owned(),
                image: IMAGE.to_owned(),
                volume_name: VOLUME_NAME.to_owned(),
                volume_expected: true,
                volume_identity: volume_identity.to_owned(),
                ..OwnershipJournal::default()
            };
            journal.persist(&root).unwrap();
            drop(owner);

            OracleFixture::cleanup_existing(FixtureConfig {
                docker_program: docker,
                docker_host: "fake".to_owned(),
                root: root.clone(),
                startup_timeout_seconds: 1,
            })
            .unwrap();

            assert!(!root.exists());
        }
    }

    #[test]
    fn cleanup_existing_retains_container_identity_label_and_image_mismatches() {
        for (current_id, current_label, current_image) in [
            ("foreign-id", "token", IMAGE),
            ("container-id", "foreign", IMAGE),
            ("container-id", "token", "foreign-image"),
        ] {
            let directory = tempdir().unwrap();
            let root = directory.path().join("fixture");
            let docker = directory.path().join("docker-fake");
            let owner = OwnerClaim::acquire(&root).unwrap();
            let token = owner.token.clone();
            write_script(
                &docker,
                &format!(
                    "#!/bin/sh\n\
                     shift 2\n\
                     if [ \"$1\" = ps ]; then echo {CONTAINER_NAME}; exit 0; fi\n\
                     if [ \"$1\" = inspect ]; then\n\
                       case \"$*\" in\n\
                         *Config.Image*) echo '{current_image}' ;;\n\
                         *Config.Labels*) echo '{current_label}' ;;\n\
                         *) echo '{current_id}' ;;\n\
                       esac\n\
                       exit 0\n\
                     fi\n\
                     exit 0\n"
                )
                .replace("echo 'token'", &format!("echo '{token}'")),
            );
            let journal = OwnershipJournal {
                token,
                container_name: CONTAINER_NAME.to_owned(),
                container_expected: true,
                container_id: "container-id".to_owned(),
                image: IMAGE.to_owned(),
                ..OwnershipJournal::default()
            };
            journal.persist(&root).unwrap();
            drop(owner);

            assert!(
                OracleFixture::cleanup_existing(FixtureConfig {
                    docker_program: docker,
                    docker_host: "fake".to_owned(),
                    root: root.clone(),
                    startup_timeout_seconds: 1,
                })
                .is_err()
            );
            assert!(root.join("journal.json").exists());
        }
    }

    #[test]
    fn cleanup_existing_retains_foreign_network_and_volume() {
        for kind in ["network", "volume"] {
            let directory = tempdir().unwrap();
            let root = directory.path().join("fixture");
            let docker = directory.path().join("docker-fake");
            let owner = OwnerClaim::acquire(&root).unwrap();
            write_script(
                &docker,
                &format!(
                    "#!/bin/sh\n\
                     shift 2\n\
                     if [ \"$1\" = ps ]; then exit 0; fi\n\
                     if [ \"$1\" = network ] && [ \"$2\" = ls ]; then [ '{kind}' = network ] && echo {NETWORK_NAME}; exit 0; fi\n\
                     if [ \"$1\" = network ] && [ \"$2\" = inspect ]; then\n\
                       case \"$*\" in *Labels*) echo foreign ;; *) echo network-id ;; esac; exit 0\n\
                     fi\n\
                     if [ \"$1\" = volume ] && [ \"$2\" = ls ]; then [ '{kind}' = volume ] && echo {VOLUME_NAME}; exit 0; fi\n\
                     if [ \"$1\" = volume ] && [ \"$2\" = inspect ]; then\n\
                       case \"$*\" in *Labels*) echo foreign ;; *) echo 'created|/mount|local' ;; esac; exit 0\n\
                     fi\n\
                     exit 0\n"
                ),
            );
            let journal = OwnershipJournal {
                token: owner.token.clone(),
                network_name: NETWORK_NAME.to_owned(),
                network_expected: kind == "network",
                network_id: "network-id".to_owned(),
                volume_name: VOLUME_NAME.to_owned(),
                volume_expected: kind == "volume",
                volume_identity: "created|/mount|local".to_owned(),
                ..OwnershipJournal::default()
            };
            journal.persist(&root).unwrap();
            drop(owner);

            assert!(
                OracleFixture::cleanup_existing(FixtureConfig {
                    docker_program: docker,
                    docker_host: "fake".to_owned(),
                    root: root.clone(),
                    startup_timeout_seconds: 1,
                })
                .is_err()
            );
            assert!(root.join("journal.json").exists());
        }
    }

    #[test]
    fn resource_evidence_revalidates_container_volume_and_network() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("fixture");
        let docker = directory.path().join("docker-fake");
        write_resource_inspection_script(&docker, VOLUME_NAME);
        let owner = OwnerClaim::acquire(&root).unwrap();
        let journal = OwnershipJournal {
            token: "token".to_owned(),
            network_name: NETWORK_NAME.to_owned(),
            network_expected: true,
            network_id: "network-id".to_owned(),
            volume_name: VOLUME_NAME.to_owned(),
            volume_expected: true,
            volume_identity: "created|/mount|local".to_owned(),
            container_name: CONTAINER_NAME.to_owned(),
            container_expected: true,
            container_id: "container-id".to_owned(),
            image: IMAGE.to_owned(),
            host_port: 15_221,
            ..OwnershipJournal::default()
        };
        let journal_identity = journal.persist(&root).unwrap();
        let mut fixture = test_fixture(root, docker, owner, journal, journal_identity);

        let evidence = fixture.resource_evidence().unwrap();

        assert_eq!(evidence.container_id, "container-id");
        assert_eq!(evidence.container_owner_label, "token");
        assert_eq!(evidence.network_id, "network-id");
        assert_eq!(evidence.network_owner_label, "token");
        assert_eq!(evidence.attached_network_id, "network-id");
        assert_eq!(evidence.volume_identity, "created|/mount|local");
        assert_eq!(evidence.volume_name, VOLUME_NAME);
        assert_eq!(evidence.volume_owner_label, "token");
        assert_eq!(evidence.host_port, 15_221);
    }

    #[test]
    fn resource_evidence_rejects_replaced_volume_attachment() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("fixture");
        let docker = directory.path().join("docker-fake");
        write_resource_inspection_script(&docker, "foreign-volume");
        let owner = OwnerClaim::acquire(&root).unwrap();
        let journal = OwnershipJournal {
            token: "token".to_owned(),
            network_name: NETWORK_NAME.to_owned(),
            network_expected: true,
            network_id: "network-id".to_owned(),
            volume_name: VOLUME_NAME.to_owned(),
            volume_expected: true,
            volume_identity: "created|/mount|local".to_owned(),
            container_name: CONTAINER_NAME.to_owned(),
            container_expected: true,
            container_id: "container-id".to_owned(),
            image: IMAGE.to_owned(),
            host_port: 15_221,
            ..OwnershipJournal::default()
        };
        let journal_identity = journal.persist(&root).unwrap();
        let mut fixture = test_fixture(root, docker, owner, journal, journal_identity);

        assert!(fixture.resource_evidence().is_err());
    }

    fn test_fixture(
        root: PathBuf,
        docker_program: PathBuf,
        owner: OwnerClaim,
        journal: OwnershipJournal,
        journal_identity: FileIdentity,
    ) -> OracleFixture {
        OracleFixture {
            config: FixtureConfig {
                docker_program,
                docker_host: "fake".to_owned(),
                root,
                startup_timeout_seconds: 1,
            },
            owner: Some(owner),
            journal,
            journal_identity: Some(journal_identity),
            credentials: None,
            database_id: 0,
            database_unique_name: String::new(),
            database_incarnation: None,
            trace_identity: Cell::new(None),
            operation_deadline: Cell::new(None),
            cleanup_attempted: true,
        }
    }

    fn write_script(path: &Path, contents: &str) {
        std::fs::write(path, contents).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn write_resource_inspection_script(path: &Path, mounted_volume: &str) {
        write_script(
            path,
            &format!(
                "#!/bin/sh\n\
                 shift 2\n\
                 if [ \"$1\" = inspect ]; then\n\
                   case \"$*\" in\n\
                     *Config.Image*) echo '{IMAGE}' ;;\n\
                     *Config.Labels*) echo token ;;\n\
                     *Mounts*) echo '{mounted_volume}' ;;\n\
                     *NetworkSettings*) echo network-id ;;\n\
                     *) echo container-id ;;\n\
                   esac\n\
                   exit 0\n\
                 fi\n\
                 if [ \"$1\" = network ] && [ \"$2\" = inspect ]; then\n\
                   case \"$*\" in\n\
                     *Labels*) echo token ;;\n\
                     *) echo network-id ;;\n\
                   esac\n\
                   exit 0\n\
                 fi\n\
                 if [ \"$1\" = volume ] && [ \"$2\" = inspect ]; then\n\
                   case \"$*\" in\n\
                     *Labels*) echo token ;;\n\
                     *) echo 'created|/mount|local' ;;\n\
                   esac\n\
                   exit 0\n\
                 fi\n\
                 if [ \"$1\" = port ]; then echo '127.0.0.1:15221'; exit 0; fi\n\
                 exit 0\n"
            ),
        );
    }
}
