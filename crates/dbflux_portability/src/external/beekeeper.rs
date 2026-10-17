/// Beekeeper Studio `app.db` importer.
///
/// Beekeeper Studio persists saved connections as a TypeORM entity in its own
/// SQLite database. The table is expected to be named `saved_connection`, but
/// column and table naming has drifted across Beekeeper releases, so the
/// table is located by its column signature rather than assumed by name.
///
/// Saved passwords and SSH secrets are encrypted with a key generated per
/// Beekeeper install. That key lives in a `.key` file next to `app.db`,
/// itself encrypted with a fixed key from Beekeeper's source code, so the
/// importer recovers the secrets when it reads the database of an install on
/// this machine. A database copied without its `.key`, or whose key does not
/// decrypt, still imports: only the secrets are skipped, with a reason telling
/// the user to re-enter them.
use std::path::{Path, PathBuf};

use dbflux_core::{DbConfig, DbKind, SshAuthMethod, SshTunnelConfig};
use rusqlite::Connection;
use secrecy::SecretString;

use super::{
    ConnectionImporter, ExternalImportCandidate, ExternalImportError, ExternalImportOutcome,
    ExternalImportSkip, ImportInput,
};

/// Column names this importer looks for, tried in order per logical field to
/// tolerate naming drift between Beekeeper Studio versions.
struct ColumnSet {
    name: String,
    connection_type: String,
    host: Option<String>,
    port: Option<String>,
    username: Option<String>,
    default_database: Option<String>,
    socket_path: Option<String>,
    password: Option<String>,
    ssh_enabled: Option<String>,
    ssh_host: Option<String>,
    ssh_port: Option<String>,
    ssh_username: Option<String>,
    ssh_mode: Option<String>,
    ssh_keyfile: Option<String>,
    ssh_password: Option<String>,
    ssh_keyfile_password: Option<String>,
    ssh_bastion_host: Option<String>,
}

/// One saved-connection row mapped to DBFlux, with its secrets still in
/// Beekeeper's encrypted form.
struct MappedRow {
    config: DbConfig,
    kind: DbKind,
    encrypted_password: Option<String>,
    encrypted_ssh_secret: Option<String>,
    notes: Vec<String>,
}

impl MappedRow {
    fn into_candidate(self, name: String, install_key: Option<&str>) -> ExternalImportCandidate {
        let mut reasons = self.notes;
        let secret = recover_secret(
            self.encrypted_password,
            install_key,
            "password",
            &mut reasons,
        );
        let ssh_secret = recover_secret(
            self.encrypted_ssh_secret,
            install_key,
            "SSH secret",
            &mut reasons,
        );

        ExternalImportCandidate {
            name,
            config: self.config,
            kind: self.kind,
            secret,
            ssh_secret,
            secret_skip_reason: (!reasons.is_empty()).then(|| reasons.join("; ")),
        }
    }
}

pub struct BeekeeperImporter;

impl ConnectionImporter for BeekeeperImporter {
    fn id(&self) -> &'static str {
        "beekeeper"
    }

    fn display_name(&self) -> &'static str {
        "Beekeeper Studio"
    }

    fn parse(&self, input: &ImportInput<'_>) -> Result<ExternalImportOutcome, ExternalImportError> {
        let path = input.primary.as_path()?;

        let conn =
            Connection::open(path).map_err(|e| ExternalImportError::Sqlite(e.to_string()))?;

        let table = find_connection_table(&conn)?;
        let columns = resolve_columns(&conn, &table)?;
        let install_key = load_install_key(path);

        let query = format!("SELECT * FROM \"{table}\"");
        let mut stmt = conn
            .prepare(&query)
            .map_err(|e| ExternalImportError::Sqlite(e.to_string()))?;

        let mut outcome = ExternalImportOutcome::default();

        let mut rows = stmt
            .query([])
            .map_err(|e| ExternalImportError::Sqlite(e.to_string()))?;

        while let Some(row) = rows
            .next()
            .map_err(|e| ExternalImportError::Sqlite(e.to_string()))?
        {
            let name: String = get_text(row, &columns.name).unwrap_or_default();
            let connection_type = get_text(row, &columns.connection_type).unwrap_or_default();

            match map_row(row, &columns, &connection_type) {
                Ok(mapped) => outcome
                    .candidates
                    .push(mapped.into_candidate(name, install_key.as_deref())),
                Err(reason) => outcome.skips.push(ExternalImportSkip { name, reason }),
            }
        }

        Ok(outcome)
    }
}

/// Locates the saved-connections table by column signature: a table with at
/// least `name`, a connection-type-like column, and `host` is treated as the
/// saved-connections table regardless of its exact name.
fn find_connection_table(conn: &Connection) -> Result<String, ExternalImportError> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .map_err(|e| ExternalImportError::Sqlite(e.to_string()))?;

    let table_names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| ExternalImportError::Sqlite(e.to_string()))?
        .collect::<Result<_, _>>()
        .map_err(|e| ExternalImportError::Sqlite(e.to_string()))?;

    for table_name in &table_names {
        let columns = table_columns(conn, table_name)?;
        let has_name = find_column(&columns, &["name"]).is_some();
        let has_type = find_column(&columns, &["connectionType", "connection_type"]).is_some();
        let has_host = find_column(&columns, &["host"]).is_some();

        if has_name && has_type && has_host {
            return Ok(table_name.clone());
        }
    }

    Err(ExternalImportError::NotBeekeeperDatabase(
        "no table with name/connectionType/host columns was found".to_string(),
    ))
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>, ExternalImportError> {
    let query = format!("PRAGMA table_info(\"{table}\")");
    let mut stmt = conn
        .prepare(&query)
        .map_err(|e| ExternalImportError::Sqlite(e.to_string()))?;

    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| ExternalImportError::Sqlite(e.to_string()))?
        .collect::<Result<_, _>>()
        .map_err(|e| ExternalImportError::Sqlite(e.to_string()))?;

    Ok(columns)
}

/// Returns the first candidate name present in `columns`, case-insensitively.
fn find_column(columns: &[String], candidates: &[&str]) -> Option<String> {
    candidates.iter().find_map(|candidate| {
        columns
            .iter()
            .find(|c| c.eq_ignore_ascii_case(candidate))
            .cloned()
    })
}

fn resolve_columns(conn: &Connection, table: &str) -> Result<ColumnSet, ExternalImportError> {
    let columns = table_columns(conn, table)?;

    let name = find_column(&columns, &["name"])
        .ok_or_else(|| ExternalImportError::NotBeekeeperDatabase("missing 'name' column".into()))?;
    let connection_type = find_column(&columns, &["connectionType", "connection_type"])
        .ok_or_else(|| {
            ExternalImportError::NotBeekeeperDatabase("missing 'connectionType' column".into())
        })?;

    Ok(ColumnSet {
        name,
        connection_type,
        host: find_column(&columns, &["host"]),
        port: find_column(&columns, &["port"]),
        username: find_column(&columns, &["username"]),
        default_database: find_column(&columns, &["defaultDatabase", "default_database"]),
        socket_path: find_column(&columns, &["socketPath", "socket_path"]),
        password: find_column(&columns, &["password"]),
        ssh_enabled: find_column(&columns, &["sshEnabled", "ssh_enabled"]),
        ssh_host: find_column(&columns, &["sshHost", "ssh_host"]),
        ssh_port: find_column(&columns, &["sshPort", "ssh_port"]),
        ssh_username: find_column(&columns, &["sshUsername", "ssh_username"]),
        ssh_mode: find_column(&columns, &["sshMode", "ssh_mode"]),
        ssh_keyfile: find_column(&columns, &["sshKeyfile", "ssh_keyfile"]),
        ssh_password: find_column(&columns, &["sshPassword", "ssh_password"]),
        ssh_keyfile_password: find_column(
            &columns,
            &["sshKeyfilePassword", "ssh_keyfile_password"],
        ),
        ssh_bastion_host: find_column(&columns, &["sshBastionHost", "ssh_bastion_host"]),
    })
}

fn get_text(row: &rusqlite::Row<'_>, column: &str) -> Option<String> {
    row.get::<_, Option<String>>(column).ok().flatten()
}

fn get_port(row: &rusqlite::Row<'_>, column: &Option<String>) -> Option<u16> {
    let column = column.as_ref()?;

    if let Ok(Some(text)) = row.get::<_, Option<String>>(column.as_str())
        && let Ok(port) = text.trim().parse::<u16>()
    {
        return Some(port);
    }

    row.get::<_, Option<i64>>(column.as_str())
        .ok()
        .flatten()
        .and_then(|v| u16::try_from(v).ok())
}

fn get_opt_text(row: &rusqlite::Row<'_>, column: &Option<String>) -> Option<String> {
    let column = column.as_ref()?;
    get_text(row, column).filter(|v| !v.is_empty())
}

fn get_flag(row: &rusqlite::Row<'_>, column: &Option<String>) -> bool {
    let Some(column) = column.as_ref() else {
        return false;
    };

    match row.get::<_, Option<i64>>(column.as_str()) {
        Ok(value) => value.unwrap_or(0) != 0,
        Err(_) => get_text(row, column).is_some_and(|text| matches!(text.trim(), "1" | "true")),
    }
}

/// Maps Beekeeper's SSH columns to an inline tunnel. Returns the tunnel, the
/// encrypted secret its authentication mode needs, and notes for settings
/// DBFlux cannot represent.
fn map_ssh_tunnel(
    row: &rusqlite::Row<'_>,
    columns: &ColumnSet,
) -> (Option<SshTunnelConfig>, Option<String>, Vec<String>) {
    if !get_flag(row, &columns.ssh_enabled) {
        return (None, None, Vec::new());
    }
    let Some(host) = get_opt_text(row, &columns.ssh_host) else {
        return (None, None, Vec::new());
    };

    let (auth_method, secret_column) = match get_opt_text(row, &columns.ssh_mode).as_deref() {
        Some("keyfile") => (
            SshAuthMethod::PrivateKey {
                key_path: get_opt_text(row, &columns.ssh_keyfile).map(PathBuf::from),
            },
            columns.ssh_keyfile_password.clone(),
        ),
        Some("userpass") => (SshAuthMethod::Password, columns.ssh_password.clone()),
        _ => (SshAuthMethod::PrivateKey { key_path: None }, None),
    };

    let mut notes = Vec::new();
    if let Some(bastion) = get_opt_text(row, &columns.ssh_bastion_host) {
        notes.push(format!(
            "SSH bastion host '{bastion}' is not imported; add it to the tunnel after import"
        ));
    }

    let tunnel = SshTunnelConfig {
        host,
        port: get_port(row, &columns.ssh_port).unwrap_or(22),
        user: get_opt_text(row, &columns.ssh_username).unwrap_or_default(),
        auth_method,
        ssh_config_host: None,
    };

    (Some(tunnel), get_opt_text(row, &secret_column), notes)
}

/// Maps one saved-connection row, keeping its secrets encrypted.
fn map_row(
    row: &rusqlite::Row<'_>,
    columns: &ColumnSet,
    connection_type: &str,
) -> Result<MappedRow, String> {
    let encrypted_password = get_opt_text(row, &columns.password);
    let (ssh_tunnel, encrypted_ssh_secret, notes) = map_ssh_tunnel(row, columns);

    let host = get_opt_text(row, &columns.host);
    let port = get_port(row, &columns.port);
    let username = get_opt_text(row, &columns.username);
    let database = get_opt_text(row, &columns.default_database);
    let socket_path = get_opt_text(row, &columns.socket_path);

    let (config, kind) = match connection_type {
        "postgresql" => (
            DbConfig::Postgres {
                use_uri: false,
                uri: None,
                host: host.ok_or_else(|| "missing required field 'host'".to_string())?,
                port: port.unwrap_or(5432),
                user: username.unwrap_or_default(),
                database: database.unwrap_or_default(),
                ssl_mode: None,
                ssl_root_cert_path: None,
                ssl_client_cert_path: None,
                ssl_client_key_path: None,
                ssh_tunnel: ssh_tunnel.clone(),
                ssh_tunnel_profile_id: None,
            },
            DbKind::Postgres,
        ),

        "mysql" | "mariadb" => {
            let kind = if connection_type == "mariadb" {
                DbKind::MariaDB
            } else {
                DbKind::MySQL
            };
            (
                DbConfig::MySQL {
                    use_uri: false,
                    uri: None,
                    host: host.ok_or_else(|| "missing required field 'host'".to_string())?,
                    port: port.unwrap_or(3306),
                    user: username.unwrap_or_default(),
                    database,
                    ssl_mode: None,
                    ssl_root_cert_path: None,
                    ssl_client_cert_path: None,
                    ssl_client_key_path: None,
                    ssh_tunnel: ssh_tunnel.clone(),
                    ssh_tunnel_profile_id: None,
                },
                kind,
            )
        }

        "sqlite" => {
            let path = database
                .or(socket_path)
                .ok_or_else(|| "sqlite connection has no database file path".to_string())?;
            (
                DbConfig::SQLite {
                    path: path.into(),
                    connection_id: None,
                },
                DbKind::SQLite,
            )
        }

        "sqlserver" => (
            DbConfig::SqlServer {
                use_uri: false,
                uri: None,
                host: host.ok_or_else(|| "missing required field 'host'".to_string())?,
                port: port.unwrap_or(1433),
                user: username.unwrap_or_default(),
                database,
                instance: None,
                ssl_mode: None,
                trust_server_certificate: false,
                ssl_root_cert_path: None,
                ssh_tunnel: ssh_tunnel.clone(),
                ssh_tunnel_profile_id: None,
            },
            DbKind::SqlServer,
        ),

        "mongodb" => (
            DbConfig::MongoDB {
                use_uri: false,
                uri: None,
                host: host.ok_or_else(|| "missing required field 'host'".to_string())?,
                port: port.unwrap_or(27017),
                user: username,
                database,
                auth_database: None,
                ssl_mode: None,
                ssl_root_cert_path: None,
                ssl_client_cert_path: None,
                ssl_client_key_path: None,
                ssh_tunnel: ssh_tunnel.clone(),
                ssh_tunnel_profile_id: None,
            },
            DbKind::MongoDB,
        ),

        "redis" => (
            DbConfig::Redis {
                use_uri: false,
                uri: None,
                host: host.ok_or_else(|| "missing required field 'host'".to_string())?,
                port: port.unwrap_or(6379),
                user: username,
                database: None,
                tls: false,
                ssl_mode: None,
                ssl_root_cert_path: None,
                ssl_client_cert_path: None,
                ssl_client_key_path: None,
                ssh_tunnel: ssh_tunnel.clone(),
                ssh_tunnel_profile_id: None,
                topology: None,
                sentinel_master_name: None,
                additional_nodes: None,
            },
            DbKind::Redis,
        ),

        other => return Err(format!("unsupported connectionType '{other}'")),
    };

    Ok(MappedRow {
        config,
        kind,
        encrypted_password,
        encrypted_ssh_secret,
        notes,
    })
}

fn recover_secret(
    encrypted: Option<String>,
    install_key: Option<&str>,
    label: &str,
    reasons: &mut Vec<String>,
) -> Option<SecretString> {
    let encrypted = encrypted?;
    let plaintext = install_key.and_then(|key| decrypt_string(key, &encrypted));

    if plaintext.is_none() {
        reasons.push(format!(
            "{label} is encrypted with the key of another Beekeeper install; re-enter it after import"
        ));
    }

    plaintext.map(SecretString::from)
}

/// Key Beekeeper Studio uses to encrypt the per-install key in its `.key`
/// file (`defaultEncryptionKey` in `src/common/encryption_key.ts`).
const BEEKEEPER_DEFAULT_KEY: &str =
    "38782F413F442A472D4B6150645367566B59703373367639792442264529482B";

/// Reads the per-install key from the `.key` file Beekeeper keeps next to
/// `app.db`. `None` when the file is absent or does not decrypt.
fn load_install_key(database_path: &Path) -> Option<String> {
    let key_file = database_path.parent()?.join(".key");
    let encrypted = std::fs::read_to_string(key_file).ok()?;

    match decrypt_json(BEEKEEPER_DEFAULT_KEY, encrypted.trim())? {
        serde_json::Value::Object(fields) => {
            fields.get("encryptionKey")?.as_str().map(str::to_string)
        }
        _ => None,
    }
}

fn decrypt_string(key: &str, ciphertext: &str) -> Option<String> {
    match decrypt_json(key, ciphertext)? {
        serde_json::Value::String(value) => Some(value),
        _ => None,
    }
}

/// Decrypts the `simple-encryptor` format Beekeeper uses: a hex HMAC-SHA256
/// of the rest, a hex IV, then base64 AES-256-CBC of a JSON document, all
/// keyed by the SHA-256 of the key string. `None` on any mismatch.
fn decrypt_json(key: &str, ciphertext: &str) -> Option<serde_json::Value> {
    use aes::Aes256;
    use base64::Engine as _;
    use cbc::Decryptor;
    use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::{Digest, Sha256};

    let derived_key = Sha256::digest(key.as_bytes());
    let expected_hmac = hex::decode(ciphertext.get(..64)?).ok()?;
    let body = ciphertext.get(64..)?;

    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(derived_key.as_slice()).ok()?;
    mac.update(body.as_bytes());
    mac.verify_slice(&expected_hmac).ok()?;

    let iv = hex::decode(body.get(..32)?).ok()?;
    let encrypted = base64::engine::general_purpose::STANDARD
        .decode(body.get(32..)?)
        .ok()?;
    let plaintext = Decryptor::<Aes256>::new_from_slices(derived_key.as_slice(), &iv)
        .ok()?
        .decrypt_padded_vec_mut::<Pkcs7>(&encrypted)
        .ok()?;

    serde_json::from_slice(&plaintext).ok()
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::external::{ImportSource, importers};
    use tempfile::NamedTempFile;

    fn seed_db(rows: &[(&str, &str, &str, i64, &str, &str, &str)]) -> NamedTempFile {
        let file = NamedTempFile::new().expect("temp file");
        let conn = Connection::open(file.path()).expect("open");

        conn.execute(
            "CREATE TABLE saved_connection (
                id INTEGER PRIMARY KEY,
                name TEXT,
                connectionType TEXT,
                host TEXT,
                port INTEGER,
                username TEXT,
                defaultDatabase TEXT,
                socketPath TEXT,
                password TEXT,
                ssl INTEGER,
                readOnlyMode INTEGER
            )",
            [],
        )
        .expect("create table");

        for (name, connection_type, host, port, username, default_database, password) in rows {
            conn.execute(
                "INSERT INTO saved_connection
                    (name, connectionType, host, port, username, defaultDatabase, password)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    name,
                    connection_type,
                    host,
                    port,
                    username,
                    default_database,
                    password
                ],
            )
            .expect("insert row");
        }

        file
    }

    #[test]
    fn registry_includes_beekeeper() {
        assert!(importers().iter().any(|i| i.id() == "beekeeper"));
    }

    #[test]
    fn happy_path_postgres_connection() {
        let file = seed_db(&[(
            "Demo Postgres",
            "postgresql",
            "db.example.invalid",
            5432,
            "demo",
            "app",
            "",
        )]);

        let source = ImportSource::Path(file.path().to_path_buf());
        let outcome = BeekeeperImporter
            .parse(&ImportInput::new(&source))
            .expect("parse");

        assert!(outcome.skips.is_empty());
        assert_eq!(outcome.candidates.len(), 1);
        let candidate = outcome.candidates.first().expect("candidate");
        assert_eq!(candidate.name, "Demo Postgres");
        assert_eq!(candidate.kind, DbKind::Postgres);
        assert!(candidate.secret_skip_reason.is_none());
        match &candidate.config {
            DbConfig::Postgres {
                host,
                port,
                user,
                database,
                ..
            } => {
                assert_eq!(host, "db.example.invalid");
                assert_eq!(*port, 5432);
                assert_eq!(user, "demo");
                assert_eq!(database, "app");
            }
            other => panic!("expected Postgres config, got {other:?}"),
        }
    }

    #[test]
    fn encrypted_password_still_imports_connection_with_secret_skip() {
        let file = seed_db(&[(
            "Demo MySQL",
            "mysql",
            "db.example.invalid",
            3306,
            "demo",
            "app",
            "{\"iv\":\"...\",\"ciphertext\":\"...\"}",
        )]);

        let source = ImportSource::Path(file.path().to_path_buf());
        let outcome = BeekeeperImporter
            .parse(&ImportInput::new(&source))
            .expect("parse");

        assert_eq!(outcome.candidates.len(), 1);
        let candidate = outcome.candidates.first().expect("candidate");
        assert!(candidate.secret.is_none());
        assert!(candidate.secret_skip_reason.is_some());
    }

    #[test]
    fn unsupported_connection_type_is_skipped_with_reason() {
        let file = seed_db(&[(
            "Legacy Cassandra",
            "cassandra",
            "db.example.invalid",
            9042,
            "demo",
            "",
            "",
        )]);

        let source = ImportSource::Path(file.path().to_path_buf());
        let outcome = BeekeeperImporter
            .parse(&ImportInput::new(&source))
            .expect("parse");

        assert!(outcome.candidates.is_empty());
        assert_eq!(outcome.skips.len(), 1);
        assert!(
            outcome
                .skips
                .first()
                .expect("skip")
                .reason
                .contains("cassandra")
        );
    }

    #[test]
    fn non_beekeeper_sqlite_returns_typed_error() {
        let file = NamedTempFile::new().expect("temp file");
        let conn = Connection::open(file.path()).expect("open");
        conn.execute("CREATE TABLE unrelated_table (id INTEGER PRIMARY KEY)", [])
            .expect("create table");

        let source = ImportSource::Path(file.path().to_path_buf());
        let result = BeekeeperImporter.parse(&ImportInput::new(&source));

        assert!(matches!(
            result,
            Err(ExternalImportError::NotBeekeeperDatabase(_))
        ));
    }

    #[test]
    fn one_bad_entry_does_not_abort_the_batch() {
        let file = seed_db(&[
            (
                "Good Postgres",
                "postgresql",
                "db.example.invalid",
                5432,
                "demo",
                "app",
                "",
            ),
            (
                "Bad Cassandra",
                "cassandra",
                "db.example.invalid",
                9042,
                "demo",
                "",
                "",
            ),
        ]);

        let source = ImportSource::Path(file.path().to_path_buf());
        let outcome = BeekeeperImporter
            .parse(&ImportInput::new(&source))
            .expect("parse");

        assert_eq!(outcome.candidates.len(), 1);
        assert_eq!(outcome.skips.len(), 1);
    }

    // Produced by Beekeeper's own `simple-encryptor` 3.0.0 package: the
    // install key `0123456789abcdef0123456789abcdef` encrypted with
    // `BEEKEEPER_DEFAULT_KEY` and with a test key, then two values encrypted
    // with that install key.
    const TEST_INSTALL_KEY: &str = "0123456789abcdef0123456789abcdef";
    const KEY_FILE_WITH_DEFAULT_KEY: &str = "55cfa38c4d6a3797b69cc2ac00bb47d5cc6c0c9ad14b1b6d9110491a5a53e90270f5b40c1e3902a97f4c8dbcc615ca68w4zujadfmNupe0U7SfF+HGcwX0hH+4jdiHVWTafem8J3Uf7KtNnjWoRaX3lAPfyup0FP0QYgnz/PDo3wnL9bmA==";
    const KEY_FILE_WITH_TEST_KEY: &str = "1ba1d20d11065cf3f89b98035567885a2b50ffb621c9504147df27c17a2759c84e7a107ff7e9cde9ac375590d7957ee6vHsPYOmzr+aq7CVB5YBs7tPq8cuGEcIPAo/3AyYmwRD2Wj88F2zfdFeyUb84b0I4BLnUDLG1srdnwMfqLu8/3Q==";
    const ENCRYPTED_PASSWORD: &str = "4e087ecb9bf636228d320f31b7bc9f211d86deb4c471dfd3df5916fb482711827d9c863d99b588d2135ab541fdf28a24sTAsMEFntIoDm2ZFKVUH4g==";
    const ENCRYPTED_PASSPHRASE: &str = "0544ebbc7f7bc59bffdedc1254a64c35c33ea8074ae1b8f715f878d77f5efde4844d8a86e410191047eae0f4ab684534sk6uk9oo9qxaxcPEc0HgOLHqbkz+xE8t+1QuHqCCY+Q=";

    struct SshRow<'a> {
        mode: &'a str,
        keyfile: &'a str,
        password: Option<&'a str>,
        keyfile_password: Option<&'a str>,
        bastion_host: &'a str,
    }

    fn seed_ssh_db(directory: &std::path::Path, ssh: &SshRow<'_>) -> std::path::PathBuf {
        let path = directory.join("app.db");
        let conn = Connection::open(&path).expect("open");

        conn.execute(
            "CREATE TABLE saved_connection (
                id INTEGER PRIMARY KEY,
                name TEXT,
                connectionType TEXT,
                host TEXT,
                port INTEGER,
                username TEXT,
                defaultDatabase TEXT,
                password TEXT,
                sshEnabled BOOLEAN,
                sshHost TEXT,
                sshPort INTEGER,
                sshUsername TEXT,
                sshMode TEXT,
                sshKeyfile TEXT,
                sshKeyfilePassword TEXT,
                sshPassword TEXT,
                sshBastionHost TEXT
            )",
            [],
        )
        .expect("create table");

        conn.execute(
            "INSERT INTO saved_connection
                (name, connectionType, host, port, username, defaultDatabase, password,
                 sshEnabled, sshHost, sshPort, sshUsername, sshMode, sshKeyfile,
                 sshKeyfilePassword, sshPassword, sshBastionHost)
             VALUES ('Tunnelled', 'postgresql', '10.0.0.5', 5432, 'demo', '', ?1,
                     1, 'bastion.example.invalid', NULL, 'deploy', ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                ENCRYPTED_PASSWORD,
                ssh.mode,
                ssh.keyfile,
                ssh.keyfile_password,
                ssh.password,
                ssh.bastion_host
            ],
        )
        .expect("insert row");

        path
    }

    fn parse_single(path: std::path::PathBuf) -> ExternalImportCandidate {
        let source = ImportSource::Path(path);
        let mut outcome = BeekeeperImporter
            .parse(&ImportInput::new(&source))
            .expect("parse");
        assert!(outcome.skips.is_empty());
        assert_eq!(outcome.candidates.len(), 1);
        outcome.candidates.remove(0)
    }

    fn keyfile_row() -> SshRow<'static> {
        SshRow {
            mode: "keyfile",
            keyfile: "/home/demo/.ssh/id_ed25519",
            password: None,
            keyfile_password: Some(ENCRYPTED_PASSPHRASE),
            bastion_host: "",
        }
    }

    #[test]
    fn decrypts_values_produced_by_simple_encryptor() {
        assert_eq!(
            decrypt_string(TEST_INSTALL_KEY, ENCRYPTED_PASSWORD).as_deref(),
            Some("s3cret-pa$$")
        );

        let key_file = decrypt_json("dbflux-test-default-key", KEY_FILE_WITH_TEST_KEY)
            .expect("key file decrypts");
        assert_eq!(key_file["encryptionKey"], TEST_INSTALL_KEY);
    }

    #[test]
    fn rejects_a_value_whose_hmac_does_not_match() {
        let tampered = format!("0{}", &ENCRYPTED_PASSWORD[1..]);
        assert!(decrypt_string(TEST_INSTALL_KEY, &tampered).is_none());
        assert!(decrypt_string("another-install-key-0000000000", ENCRYPTED_PASSWORD).is_none());
    }

    #[test]
    fn recovers_secrets_and_ssh_tunnel_from_a_local_install() {
        let directory = tempfile::tempdir().expect("temp dir");
        std::fs::write(directory.path().join(".key"), KEY_FILE_WITH_DEFAULT_KEY).expect("key");
        let candidate = parse_single(seed_ssh_db(directory.path(), &keyfile_row()));

        use secrecy::ExposeSecret;
        assert!(candidate.secret_skip_reason.is_none());
        assert_eq!(
            candidate.secret.as_ref().map(|s| s.expose_secret()),
            Some("s3cret-pa$$")
        );
        assert_eq!(
            candidate.ssh_secret.as_ref().map(|s| s.expose_secret()),
            Some("key-passphrase")
        );

        let tunnel = candidate.config.ssh_tunnel().expect("ssh tunnel");
        assert_eq!(tunnel.host, "bastion.example.invalid");
        assert_eq!(tunnel.port, 22);
        assert_eq!(tunnel.user, "deploy");
        assert_eq!(
            tunnel.auth_method,
            SshAuthMethod::PrivateKey {
                key_path: Some(PathBuf::from("/home/demo/.ssh/id_ed25519"))
            }
        );
    }

    #[test]
    fn without_the_install_key_the_tunnel_imports_and_secrets_are_skipped() {
        let directory = tempfile::tempdir().expect("temp dir");
        let candidate = parse_single(seed_ssh_db(directory.path(), &keyfile_row()));

        assert!(candidate.secret.is_none());
        assert!(candidate.ssh_secret.is_none());
        let reason = candidate.secret_skip_reason.expect("skip reason");
        assert!(reason.contains("password"), "{reason}");
        assert!(reason.contains("SSH secret"), "{reason}");
        assert!(candidate.config.ssh_tunnel().is_some());
    }

    #[test]
    fn password_mode_uses_the_ssh_password() {
        let directory = tempfile::tempdir().expect("temp dir");
        std::fs::write(directory.path().join(".key"), KEY_FILE_WITH_DEFAULT_KEY).expect("key");
        let row = SshRow {
            mode: "userpass",
            keyfile: "",
            password: Some(ENCRYPTED_PASSPHRASE),
            keyfile_password: None,
            bastion_host: "",
        };
        let candidate = parse_single(seed_ssh_db(directory.path(), &row));

        use secrecy::ExposeSecret;
        assert_eq!(
            candidate.config.ssh_tunnel().map(|t| t.auth_method.clone()),
            Some(SshAuthMethod::Password)
        );
        assert_eq!(
            candidate.ssh_secret.as_ref().map(|s| s.expose_secret()),
            Some("key-passphrase")
        );
    }

    #[test]
    fn bastion_host_is_reported_as_not_imported() {
        let directory = tempfile::tempdir().expect("temp dir");
        std::fs::write(directory.path().join(".key"), KEY_FILE_WITH_DEFAULT_KEY).expect("key");
        let row = SshRow {
            bastion_host: "jump.example.invalid",
            ..keyfile_row()
        };
        let candidate = parse_single(seed_ssh_db(directory.path(), &row));

        let reason = candidate.secret_skip_reason.expect("bastion note");
        assert!(reason.contains("jump.example.invalid"), "{reason}");
        assert!(candidate.secret.is_some());
    }
}
