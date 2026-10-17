/// Sequel Ace favorites importer.
///
/// Sequel Ace keeps its favorites in `Favorites.plist`
/// (`~/Library/Containers/com.sequel-ace.sequel-ace/Data/Library/Application
/// Support/Sequel Ace/Data/`) under a `Favorites Root` group tree, and writes
/// exports as a flat `SPConnectionFavorites` list. Both shapes are read.
///
/// Passwords are not in the file: Sequel Ace stores them in the macOS
/// keychain under names derived from the favorite (`SAKeychainNaming` in
/// Sequel Ace). The importer looks them up there, which macOS may confirm with
/// the user item by item; a password that is missing or refused simply leaves
/// the connection without one.
use std::path::PathBuf;

use dbflux_core::{DbConfig, DbKind, SshAuthMethod, SshTunnelConfig};
use plist::{Dictionary, Value};
use secrecy::SecretString;

use super::{
    ConnectionImporter, ExternalImportCandidate, ExternalImportError, ExternalImportOutcome,
    ExternalImportSkip, ImportInput,
};

/// Looks up a generic keychain password by service and account.
pub type KeychainLookup = dyn Fn(&str, &str) -> Option<SecretString> + Send + Sync;

/// Connection types from Sequel Ace's `SPConnectionType`.
const TCP_CONNECTION: i64 = 0;
const SOCKET_CONNECTION: i64 = 1;
const SSH_TUNNEL_CONNECTION: i64 = 2;
const AWS_IAM_CONNECTION: i64 = 3;

pub struct SequelAceImporter {
    keychain: Box<KeychainLookup>,
}

impl SequelAceImporter {
    /// Reads passwords from the system keychain (macOS only; none elsewhere).
    pub fn new() -> Self {
        Self::with_keychain_lookup(Box::new(system_keychain_password))
    }

    pub fn with_keychain_lookup(keychain: Box<KeychainLookup>) -> Self {
        Self { keychain }
    }
}

impl Default for SequelAceImporter {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionImporter for SequelAceImporter {
    fn id(&self) -> &'static str {
        "sequelace"
    }

    fn display_name(&self) -> &'static str {
        "Sequel Ace"
    }

    fn parse(&self, input: &ImportInput<'_>) -> Result<ExternalImportOutcome, ExternalImportError> {
        let bytes = input.primary.as_bytes()?;
        let root = Value::from_reader(std::io::Cursor::new(bytes)).map_err(|error| {
            ExternalImportError::Structure(format!("not a property list: {error}"))
        })?;

        let favorites = collect_favorites(&root)?;
        let mut outcome = ExternalImportOutcome::default();

        for favorite in favorites {
            let name = text(favorite, "name").unwrap_or_default();
            match self.map_favorite(favorite, &name) {
                Ok(candidate) => outcome.candidates.push(candidate),
                Err(reason) => outcome.skips.push(ExternalImportSkip { name, reason }),
            }
        }

        Ok(outcome)
    }
}

impl SequelAceImporter {
    fn map_favorite(
        &self,
        favorite: &Dictionary,
        name: &str,
    ) -> Result<ExternalImportCandidate, String> {
        let connection_type = integer(favorite, "type").unwrap_or(TCP_CONNECTION);
        match connection_type {
            TCP_CONNECTION | SSH_TUNNEL_CONNECTION => {}
            SOCKET_CONNECTION => return Err("socket connections are not supported".to_string()),
            AWS_IAM_CONNECTION => {
                return Err("AWS IAM authentication is not supported".to_string());
            }
            other => return Err(format!("unsupported connection type {other}")),
        }

        let host = text(favorite, "host").ok_or_else(|| "missing host".to_string())?;
        let user = text(favorite, "user").unwrap_or_default();
        let database = text(favorite, "database");
        let favorite_id = favorite_id(favorite.get("id"));

        let (ssh_tunnel, ssh_secret) = if connection_type == SSH_TUNNEL_CONNECTION {
            let (tunnel, secret) = self.map_ssh_tunnel(favorite, name, favorite_id)?;
            (Some(tunnel), secret)
        } else {
            (None, None)
        };

        let secret = match favorite_id {
            Some(id) if !name.is_empty() && !user.is_empty() => (self.keychain)(
                &format!("Sequel Ace : {name} ({id})"),
                &format!("{user}@{host}/{}", database.as_deref().unwrap_or("")),
            ),
            _ => None,
        };

        let (ssl_mode, ssl_root_cert_path, ssl_client_cert_path, ssl_client_key_path) =
            map_ssl(favorite);

        Ok(ExternalImportCandidate {
            name: name.to_string(),
            config: DbConfig::MySQL {
                use_uri: false,
                uri: None,
                host,
                port: port(favorite, "port").unwrap_or(3306),
                user,
                database,
                ssl_mode,
                ssl_root_cert_path,
                ssl_client_cert_path,
                ssl_client_key_path,
                ssh_tunnel,
                ssh_tunnel_profile_id: None,
            },
            kind: DbKind::MySQL,
            secret,
            ssh_secret,
            secret_skip_reason: None,
        })
    }

    /// Maps the SSH tunnel of a favorite. A key file selected in Sequel Ace
    /// becomes key authentication; otherwise a saved SSH password selects
    /// password authentication, and without one the agent or default keys
    /// are used.
    fn map_ssh_tunnel(
        &self,
        favorite: &Dictionary,
        name: &str,
        favorite_id: Option<i64>,
    ) -> Result<(SshTunnelConfig, Option<SecretString>), String> {
        let host = text(favorite, "sshHost").ok_or_else(|| "missing SSH host".to_string())?;
        let user = text(favorite, "sshUser").unwrap_or_default();

        let key_path = flag(favorite, "sshKeyLocationEnabled")
            .then(|| text(favorite, "sshKeyLocation"))
            .flatten()
            .map(expand_home);

        let ssh_password = match (&key_path, favorite_id) {
            (None, Some(id)) if !name.is_empty() && !user.is_empty() => (self.keychain)(
                &format!("Sequel Ace SSHTunnel : {name} ({id})"),
                &format!("{user}@{host}"),
            ),
            _ => None,
        };

        let auth_method = match (&key_path, &ssh_password) {
            (Some(path), _) => SshAuthMethod::PrivateKey {
                key_path: Some(path.clone()),
            },
            (None, Some(_)) => SshAuthMethod::Password,
            (None, None) => SshAuthMethod::PrivateKey { key_path: None },
        };

        let tunnel = SshTunnelConfig {
            host,
            port: port(favorite, "sshPort").unwrap_or(22),
            user,
            auth_method,
            ssh_config_host: None,
        };

        Ok((tunnel, ssh_password))
    }
}

/// Returns every favorite of the file, descending into groups.
fn collect_favorites(root: &Value) -> Result<Vec<&Dictionary>, ExternalImportError> {
    let not_sequel_ace = || {
        ExternalImportError::Structure(
            "not a Sequel Ace favorites file: expected 'Favorites Root' or 'SPConnectionFavorites'"
                .to_string(),
        )
    };

    let root = root.as_dictionary().ok_or_else(not_sequel_ace)?;
    let mut favorites = Vec::new();

    if let Some(group) = root.get("Favorites Root").and_then(Value::as_dictionary) {
        collect_from_group(group, &mut favorites);
    } else if let Some(list) = root.get("SPConnectionFavorites").and_then(Value::as_array) {
        collect_from_list(list, &mut favorites);
    } else {
        return Err(not_sequel_ace());
    }

    Ok(favorites)
}

fn collect_from_group<'a>(group: &'a Dictionary, favorites: &mut Vec<&'a Dictionary>) {
    if let Some(children) = group.get("Children").and_then(Value::as_array) {
        collect_from_list(children, favorites);
    }
}

fn collect_from_list<'a>(list: &'a [Value], favorites: &mut Vec<&'a Dictionary>) {
    for node in list.iter().filter_map(Value::as_dictionary) {
        if node.contains_key("Children") {
            collect_from_group(node, favorites);
        } else {
            favorites.push(node);
        }
    }
}

/// Maps Sequel Ace's SSL settings to the MySQL driver's: `useSSL` requires TLS,
/// and a CA certificate also verifies the server against it.
fn map_ssl(
    favorite: &Dictionary,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    if !flag(favorite, "useSSL") {
        return (None, None, None, None);
    }

    let enabled_path = |enabled: &str, path: &str| {
        flag(favorite, enabled)
            .then(|| text(favorite, path))
            .flatten()
            .map(|value| expand_home(value).to_string_lossy().into_owned())
    };

    let ca = enabled_path("sslCACertFileLocationEnabled", "sslCACertFileLocation");
    let cert = enabled_path(
        "sslCertificateFileLocationEnabled",
        "sslCertificateFileLocation",
    );
    let key = enabled_path("sslKeyFileLocationEnabled", "sslKeyFileLocation");
    let mode = if ca.is_some() {
        "VERIFY_CA"
    } else {
        "REQUIRED"
    };

    (Some(mode.to_string()), ca, cert, key)
}

fn text(favorite: &Dictionary, key: &str) -> Option<String> {
    favorite
        .get(key)
        .and_then(Value::as_string)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn integer(favorite: &Dictionary, key: &str) -> Option<i64> {
    match favorite.get(key)? {
        Value::Integer(value) => value.as_signed().or_else(|| {
            value
                .as_unsigned()
                .map(|v| i64::try_from(v).unwrap_or(i64::MAX))
        }),
        Value::String(value) => value.trim().parse().ok(),
        Value::Real(value) => Some(*value as i64),
        Value::Boolean(value) => Some(i64::from(*value)),
        _ => None,
    }
}

fn port(favorite: &Dictionary, key: &str) -> Option<u16> {
    integer(favorite, key).and_then(|value| u16::try_from(value).ok())
}

fn flag(favorite: &Dictionary, key: &str) -> bool {
    integer(favorite, key).is_some_and(|value| value != 0)
}

/// The favorite id the way Sequel Ace's keychain naming reads it: a number,
/// or the numeric prefix of a string (`-longLongValue`), `0` when there is
/// none. `None` only when the id is absent.
fn favorite_id(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Integer(value) => value.as_signed().or_else(|| {
            value
                .as_unsigned()
                .map(|v| i64::try_from(v).unwrap_or(i64::MAX))
        }),
        Value::Real(value) => Some(*value as i64),
        Value::String(value) => Some(long_long_value(value)),
        _ => None,
    }
}

fn long_long_value(text: &str) -> i64 {
    let trimmed = text.trim_start();
    let (sign, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let numeric: String = digits.chars().take_while(char::is_ascii_digit).collect();

    numeric
        .parse::<i64>()
        .map(|value| sign * value)
        .unwrap_or(if numeric.is_empty() {
            0
        } else if sign < 0 {
            i64::MIN
        } else {
            i64::MAX
        })
}

/// Expands a leading `~/`, which Sequel Ace stores for paths under the home
/// directory, so the SSH and TLS libraries receive an absolute path.
fn expand_home(path: String) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
}

#[cfg(target_os = "macos")]
fn system_keychain_password(service: &str, account: &str) -> Option<SecretString> {
    let entry = keyring::Entry::new(service, account).ok()?;
    entry.get_password().ok().map(SecretString::from)
}

#[cfg(not(target_os = "macos"))]
fn system_keychain_password(_service: &str, _account: &str) -> Option<SecretString> {
    None
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod tests;
