use super::*;
use crate::external::{ImportSource, importers};
use secrecy::ExposeSecret;

fn dict(entries: &[(&str, Value)]) -> Value {
    let mut dictionary = Dictionary::new();
    for (key, value) in entries {
        dictionary.insert((*key).to_string(), value.clone());
    }
    Value::Dictionary(dictionary)
}

fn s(value: &str) -> Value {
    Value::String(value.to_string())
}

fn n(value: i64) -> Value {
    Value::Integer(value.into())
}

fn plist_bytes(root: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    plist::to_writer_xml(&mut bytes, root).expect("serialize plist");
    bytes
}

/// A keychain holding exactly the given `(service, account, password)` items.
fn keychain(items: &[(&str, &str, &str)]) -> Box<KeychainLookup> {
    let items: Vec<(String, String, String)> = items
        .iter()
        .map(|(service, account, password)| {
            (
                (*service).to_string(),
                (*account).to_string(),
                (*password).to_string(),
            )
        })
        .collect();
    Box::new(move |service, account| {
        items
            .iter()
            .find(|(s, a, _)| s == service && a == account)
            .map(|(_, _, password)| SecretString::from(password.clone()))
    })
}

fn parse(root: &Value, importer: &SequelAceImporter) -> ExternalImportOutcome {
    let source = ImportSource::Bytes(plist_bytes(root));
    importer
        .parse(&ImportInput::new(&source))
        .expect("parse favorites")
}

fn favorites_root(children: Vec<Value>) -> Value {
    dict(&[(
        "Favorites Root",
        dict(&[
            ("Name", s("FAVORITES")),
            ("Children", Value::Array(children)),
        ]),
    )])
}

fn tcp_favorite() -> Value {
    dict(&[
        ("id", n(42)),
        ("name", s("Prod")),
        ("type", n(0)),
        ("host", s("db.example.invalid")),
        ("port", s("3307")),
        ("user", s("app")),
        ("database", s("shop")),
    ])
}

#[test]
fn registry_includes_sequel_ace() {
    assert!(importers().iter().any(|i| i.id() == "sequelace"));
}

#[test]
fn tcp_favorite_in_a_group_imports_with_its_keychain_password() {
    let root = favorites_root(vec![dict(&[
        ("Name", s("Work")),
        ("Children", Value::Array(vec![tcp_favorite()])),
    ])]);
    let importer = SequelAceImporter::with_keychain_lookup(keychain(&[(
        "Sequel Ace : Prod (42)",
        "app@db.example.invalid/shop",
        "s3cret",
    )]));

    let outcome = parse(&root, &importer);

    assert!(outcome.skips.is_empty());
    let candidate = outcome.candidates.first().expect("candidate");
    assert_eq!(candidate.name, "Prod");
    assert_eq!(candidate.kind, DbKind::MySQL);
    assert_eq!(
        candidate.secret.as_ref().map(|s| s.expose_secret()),
        Some("s3cret")
    );
    match &candidate.config {
        DbConfig::MySQL {
            host,
            port,
            user,
            database,
            ssh_tunnel,
            ssl_mode,
            ..
        } => {
            assert_eq!(host, "db.example.invalid");
            assert_eq!(*port, 3307);
            assert_eq!(user, "app");
            assert_eq!(database.as_deref(), Some("shop"));
            assert!(ssh_tunnel.is_none());
            assert!(ssl_mode.is_none());
        }
        other => panic!("expected MySQL config, got {other:?}"),
    }
}

#[test]
fn export_file_is_read_and_a_missing_password_is_not_an_error() {
    let root = dict(&[("SPConnectionFavorites", Value::Array(vec![tcp_favorite()]))]);
    let importer = SequelAceImporter::with_keychain_lookup(keychain(&[]));

    let outcome = parse(&root, &importer);

    let candidate = outcome.candidates.first().expect("candidate");
    assert!(candidate.secret.is_none());
    assert!(candidate.secret_skip_reason.is_none());
}

#[test]
fn ssh_favorite_with_a_key_file_maps_the_tunnel() {
    let root = favorites_root(vec![dict(&[
        ("id", s("7")),
        ("name", s("Tunnelled")),
        ("type", n(2)),
        ("host", s("10.0.0.5")),
        ("user", s("app")),
        ("sshHost", s("bastion.example.invalid")),
        ("sshUser", s("deploy")),
        ("sshKeyLocationEnabled", n(1)),
        ("sshKeyLocation", s("/keys/id_ed25519")),
    ])]);
    let importer = SequelAceImporter::with_keychain_lookup(keychain(&[]));

    let outcome = parse(&root, &importer);

    let candidate = outcome.candidates.first().expect("candidate");
    let tunnel = candidate.config.ssh_tunnel().expect("ssh tunnel");
    assert_eq!(tunnel.host, "bastion.example.invalid");
    assert_eq!(tunnel.port, 22);
    assert_eq!(tunnel.user, "deploy");
    assert_eq!(
        tunnel.auth_method,
        SshAuthMethod::PrivateKey {
            key_path: Some(PathBuf::from("/keys/id_ed25519"))
        }
    );
    assert!(candidate.ssh_secret.is_none());
}

#[test]
fn ssh_favorite_without_a_key_uses_its_saved_ssh_password() {
    let root = favorites_root(vec![dict(&[
        ("id", n(9)),
        ("name", s("Bastion")),
        ("type", n(2)),
        ("host", s("127.0.0.1")),
        ("user", s("app")),
        ("sshHost", s("bastion.example.invalid")),
        ("sshPort", n(2222)),
        ("sshUser", s("deploy")),
    ])]);
    let importer = SequelAceImporter::with_keychain_lookup(keychain(&[(
        "Sequel Ace SSHTunnel : Bastion (9)",
        "deploy@bastion.example.invalid",
        "ssh-pass",
    )]));

    let outcome = parse(&root, &importer);

    let candidate = outcome.candidates.first().expect("candidate");
    let tunnel = candidate.config.ssh_tunnel().expect("ssh tunnel");
    assert_eq!(tunnel.port, 2222);
    assert_eq!(tunnel.auth_method, SshAuthMethod::Password);
    assert_eq!(
        candidate.ssh_secret.as_ref().map(|s| s.expose_secret()),
        Some("ssh-pass")
    );
}

#[test]
fn ssl_with_a_ca_certificate_verifies_the_server() {
    let root = favorites_root(vec![dict(&[
        ("id", n(1)),
        ("name", s("TLS")),
        ("host", s("db.example.invalid")),
        ("user", s("app")),
        ("useSSL", n(1)),
        ("sslCACertFileLocationEnabled", n(1)),
        ("sslCACertFileLocation", s("/certs/ca.pem")),
        ("sslKeyFileLocationEnabled", n(0)),
        ("sslKeyFileLocation", s("/certs/ignored.key")),
    ])]);
    let importer = SequelAceImporter::with_keychain_lookup(keychain(&[]));

    let outcome = parse(&root, &importer);

    match &outcome.candidates.first().expect("candidate").config {
        DbConfig::MySQL {
            ssl_mode,
            ssl_root_cert_path,
            ssl_client_key_path,
            ..
        } => {
            assert_eq!(ssl_mode.as_deref(), Some("VERIFY_CA"));
            assert_eq!(ssl_root_cert_path.as_deref(), Some("/certs/ca.pem"));
            assert!(ssl_client_key_path.is_none());
        }
        other => panic!("expected MySQL config, got {other:?}"),
    }
}

#[test]
fn socket_and_aws_iam_favorites_are_skipped_with_a_reason() {
    let root = favorites_root(vec![
        dict(&[
            ("name", s("Local socket")),
            ("type", n(1)),
            ("socket", s("/tmp/mysql.sock")),
        ]),
        dict(&[("name", s("RDS")), ("type", n(3)), ("host", s("rds"))]),
        tcp_favorite(),
    ]);
    let importer = SequelAceImporter::with_keychain_lookup(keychain(&[]));

    let outcome = parse(&root, &importer);

    assert_eq!(outcome.candidates.len(), 1);
    let reasons: Vec<&str> = outcome.skips.iter().map(|s| s.reason.as_str()).collect();
    assert_eq!(outcome.skips.len(), 2);
    assert!(reasons.iter().any(|r| r.contains("socket")), "{reasons:?}");
    assert!(reasons.iter().any(|r| r.contains("AWS IAM")), "{reasons:?}");
}

#[test]
fn a_file_that_is_not_sequel_ace_favorites_is_rejected() {
    let importer = SequelAceImporter::with_keychain_lookup(keychain(&[]));

    let unrelated = ImportSource::Bytes(plist_bytes(&dict(&[("Other", s("value"))])));
    assert!(matches!(
        importer.parse(&ImportInput::new(&unrelated)),
        Err(ExternalImportError::Structure(_))
    ));

    let not_a_plist = ImportSource::Bytes(b"not a property list".to_vec());
    assert!(importer.parse(&ImportInput::new(&not_a_plist)).is_err());
}

#[test]
fn favorite_ids_follow_sequel_ace_keychain_naming() {
    assert_eq!(favorite_id(Some(&n(42))), Some(42));
    assert_eq!(favorite_id(Some(&s("123abc"))), Some(123));
    assert_eq!(favorite_id(Some(&s("abc"))), Some(0));
    assert_eq!(favorite_id(Some(&s("-5"))), Some(-5));
    assert_eq!(favorite_id(None), None);
}
