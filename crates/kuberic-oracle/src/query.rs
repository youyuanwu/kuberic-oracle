/// The future observer will execute fixed, read-only statements rather than
/// accepting arbitrary SQL from configuration or command-line arguments.
pub const OBSERVATION_IDENTITY_SQL: &str = r#"
SELECT
    d.dbid AS database_id,
    d.db_unique_name AS database_unique_name,
    i.instance_name AS instance_name,
    i.startup_time AS startup_time
FROM v$database d
CROSS JOIN v$instance i
"#;

pub const DATABASE_SNAPSHOT_SQL: &str = r#"
SELECT
    d.dbid AS database_id,
    d.name AS database_name,
    d.db_unique_name AS database_unique_name,
    d.database_role AS database_role,
    d.open_mode AS open_mode,
    d.current_scn AS current_scn,
    d.resetlogs_change# AS resetlogs_change,
    d.activation# AS activation_number,
    d.log_mode AS log_mode,
    d.force_logging AS force_logging,
    d.cdb AS cdb
FROM v$database d
"#;

pub const INSTANCE_SNAPSHOT_SQL: &str = r#"
SELECT
    i.instance_name AS instance_name,
    i.host_name AS host_name,
    i.version_full AS version_full,
    i.edition AS edition,
    i.startup_time AS startup_time,
    i.status AS status,
    i.database_status AS database_status
FROM v$instance i
"#;

pub const PRODUCT_COMPONENT_SQL: &str = r#"
SELECT
    p.product AS product,
    p.version AS version,
    p.version_full AS version_full,
    p.status AS release_status
FROM product_component_version p
WHERE p.product LIKE 'Oracle AI Database%'
"#;
