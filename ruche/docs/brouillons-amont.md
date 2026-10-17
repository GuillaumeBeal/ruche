# Brouillons de messages pour l'amont (DBFlux)

Ces textes sont en anglais, comme le dépôt amont. **Rien n'est publié** : chaque message part seulement après
votre accord explicite.

---

## A. Commentaire sur #773 (présentation et réservation de #1087)

> https://github.com/0xErwin1/dbflux/issues/773

```markdown
Hi! I'm Guillaume. I've started a downstream fork of DBFlux called **Ruche**
(https://github.com/GuillaumeBeal/ruche): a different name, logo and visual theme, as your
TRADEMARK.md asks for modified builds. Everything else I'd like to contribute back here
rather than carry in the fork.

Here is what I plan to work on, in this order, each as its own issue first:

1. MySQL: DECIMAL values lose precision (bug, issue coming with a failing live test).
2. Use the channel display name instead of a hard-coded "DBFlux" in the About page and the
   Linux title bar (and `%{app}` in locale strings), so nightly and downstream builds name
   themselves correctly.
3. MCP server: support SSH tunnels and proxies in the access manager.
4. MCP server: a read-only free-form SQL tool with a statement timeout (I'll check what #844
   already covered first).
5. MCP server: register the SQL Server driver.
6. #1087, foreign-key navigation in the data grid, split into the three PRs it suggests,
   if nobody has started it.
7. Transactional "save all" for grid edits, with a preview of the generated SQL (design
   discussion first).

Could I take **#1087**? And if any of these doesn't fit your plans, I'd rather know now.
Thanks for DBFlux, it's a very solid base.
```

---

## B. Issue : précision des DECIMAL MySQL (modèle « Bug report »)

**Titre** : `[bug] MySQL: DECIMAL values are decoded as floats and lose precision`

| Champ | Valeur |
|---|---|
| DBFlux version | `main` @ `f8347d93` (0.9.0-dev.0) |
| Operating system | macOS |
| Architecture | arm64 |
| Affected driver | mysql/mariadb |

**Steps to reproduce**
```markdown
1. Connect to MySQL 8.4 (the `mysql:8.4` image used by the live tests).
2. Run:
   SELECT CAST('12345678901234567.89' AS DECIMAL(20,2)) AS money,
          CAST('99999999999999999999' AS DECIMAL(20,0)) AS wide_integer;
3. Look at the values in the result grid (or in `QueryResult.rows`).
```

**Expected behavior**
```markdown
`Value::Decimal("12345678901234567.89")` and `Value::Decimal("99999999999999999999")`, exact,
as the PostgreSQL driver already does for NUMERIC.
```

**Actual behavior**
```markdown
`Value::Float(1.2345678901234568e16)`: the cents are gone and the last digits are rounded.

`mysql_value_to_value` (`crates/dbflux_driver_mysql/src/driver.rs`) has no DECIMAL branch: the
value falls through to the `i64` attempt (fails for a fractional or oversized value), then to
`f64`, which rounds anything past ~15–17 significant digits. Writing the value back from the
grid can then persist the rounded number.
```

**Logs**
```text
assertion `left == right` failed
  left: Float(1.2345678901234568e16)
 right: Decimal("12345678901234567.89")
```

**Proposed fix** (ready, with a live test): read `MYSQL_TYPE_DECIMAL` / `MYSQL_TYPE_NEWDECIMAL`
as text into `Value::Decimal` before the integer and float fallbacks.

**Labels** : `driver:bug`, `driver:mysql`

---

## C. Issue : tunnels SSH et proxys dans le serveur MCP (modèle « Feature request »)

**Titre** : `[feature] MCP: connect through SSH tunnels and proxies`

```markdown
### Problem

The MCP server's access manager refuses every profile that goes through an SSH tunnel or a
proxy: "SSH tunnels are not supported by the MCP server access manager"
(`crates/dbflux_mcp_server/src/server.rs`). Databases that are only reachable through a
bastion, which is common for production and staging, can be browsed in the app but are
invisible to an MCP client, even read-only.

### Proposed solution

- Open SSH and proxy access in the MCP server through the same `dbflux_ssh` / `dbflux_proxy`
  tunnels the app uses, owned by the MCP session and closed on `disconnect` or exit.
- Reuse the stored SSH tunnel and proxy profiles and their secrets from the keyring; the
  MCP client never sees them.
- Host-key verification behaves as in the app; an unknown host key fails the connection
  with a clear error instead of prompting, since there is no UI.

### Alternatives considered

- Asking users to open the tunnel by hand and point a second profile at localhost: works,
  but duplicates every profile and bypasses the stored tunnel settings.

### Primary area

MCP server

### Labels I plan to apply

`mcp:feature`
```
