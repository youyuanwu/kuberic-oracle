/// The future observer will execute fixed, read-only statements rather than
/// accepting arbitrary SQL from configuration or command-line arguments.
pub const DATABASE_SNAPSHOT_SQL: &str = r#"
SELECT
    d.dbid,
    d.name,
    d.db_unique_name,
    d.database_role,
    d.open_mode,
    d.current_scn,
    d.resetlogs_change#,
    d.activation#,
    d.log_mode,
    d.force_logging,
    d.cdb
FROM v$database d
"#;

pub const INSTANCE_SNAPSHOT_SQL: &str = r#"
SELECT
    i.instance_name,
    i.host_name,
    i.version_full,
    i.startup_time,
    i.status,
    i.database_status
FROM v$instance i
"#;

pub const PRODUCT_COMPONENT_SQL: &str = r#"
SELECT
    p.product,
    p.version,
    p.version_full,
    p.status
FROM product_component_version p
WHERE p.product LIKE 'Oracle Database%'
"#;
