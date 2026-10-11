/// The future observer will execute fixed, read-only statements rather than
/// accepting arbitrary SQL from configuration or command-line arguments.
pub const OBSERVATION_IDENTITY_SQL: &str = r#"
SELECT
    d.dbid AS database_id,
    d.db_unique_name AS database_unique_name,
    i.instance_name AS instance_name,
    i.startup_time AS startup_time,
    c.incarnation# AS incarnation_number,
    c.resetlogs_id AS resetlogs_id,
    c.resetlogs_change# AS resetlogs_change
FROM v$database d
CROSS JOIN v$instance i
JOIN v$database_incarnation c
  ON c.status = 'CURRENT'
 AND c.con_id = 0
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

pub const DATABASE_INCARNATION_SQL: &str = r#"
SELECT
    i.incarnation# AS incarnation_number,
    i.resetlogs_change# AS resetlogs_change,
    TO_CHAR(
        i.resetlogs_time,
        'YYYY-MM-DD"T"HH24:MI:SS',
        'NLS_DATE_LANGUAGE=American'
    ) AS resetlogs_time,
    i.prior_resetlogs_change# AS prior_resetlogs_change,
    TO_CHAR(
        i.prior_resetlogs_time,
        'YYYY-MM-DD"T"HH24:MI:SS',
        'NLS_DATE_LANGUAGE=American'
    ) AS prior_resetlogs_time,
    i.status AS status,
    i.resetlogs_id AS resetlogs_id,
    i.prior_incarnation# AS prior_incarnation_number,
    i.con_id AS container_id
FROM v$database_incarnation i
WHERE i.status = 'CURRENT'
  AND i.con_id = 0
"#;

pub const DATABASE_TARGET_IDENTITY_SQL: &str = r#"
SELECT
    d.dbid || ':' ||
    d.db_unique_name || ':' ||
    i.incarnation# || ':' ||
    i.resetlogs_id || ':' ||
    i.resetlogs_change#
FROM v$database d
CROSS JOIN v$database_incarnation i
WHERE i.status = 'CURRENT'
  AND i.con_id = 0
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
