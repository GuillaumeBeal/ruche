# Ruche — client SQL 100 % Rust natif (GPUI), inspiré de Beekeeper Studio

> Nom de travail : **Ruche** (pas de reprise du nom, du logo ni des assets Beekeeper).
> Base : rétro-ingénierie de Beekeeper Studio 5.8.1 (Electron) faite avec REA, voir la section 2.
> Révision 2 : passage de Tauri + Vue à **GPUI + gpui-component**, sans webview ni JavaScript.

## 1. Objectifs

1. **Parité fonctionnelle utile** : connexions, explorateur, éditeur SQL, édition de tables, structure, export.
2. **Même ressenti** : look sobre sombre/clair, onglets, autocomplétion contextuelle, édition en ligne avec
   changements en attente, raccourcis clavier identiques.
3. **Natif et rapide** : un seul binaire Rust, rendu GPU, démarrage instantané, résultats lus directement en
   mémoire sans sérialisation.
4. **Mieux que l'original** là où l'analyse a montré des faiblesses : secrets dans le trousseau OS, résultats
   volumineux sans troncature.
5. **Cerise** : un serveur **MCP local** pour que Claude interroge toutes les bases configurées, en lecture seule
   par défaut.

## 2. Ce que l'analyse de Beekeeper nous apprend

| Constat dans Beekeeper | Décision pour Ruche |
|---|---|
| 3 processus : main / *utility process* (tout le travail BDD) / renderer Vue 2 | **Un seul processus** : UI GPUI + cœur Rust async ; plus d'IPC du tout |
| RPC `{id, name, args}` par fenêtre, état par session `sId` | Appels directs au `SessionManager` ; une session par connexion ouverte |
| ~90 handlers `conn/*` (listTables, listTableColumns, applyChanges, alterTable…) | Trait `Connection` qui reprend cette surface (§5) |
| Requête en 2 temps : `conn/query` → id, puis `query/execute` / `query/cancel` | `QueryRegistry` + `CancellationToken` + annulation native par driver |
| Résultats tronqués à `maxResults` côté backend | La grille lit un `Arc<ResultSet>` en mémoire, sans limite artificielle (spill disque au-delà d'un seuil) |
| `selectTopStream` / `queryStream` pour l'export | Streams Rust directement vers le fichier |
| Mode transaction manuelle avec timeout | Repris tel quel |
| App DB SQLite via TypeORM (saved_connection, used_query, favorite_query, tabs, user_setting…) | SQLite + migrations, même modèle de données |
| Mots de passe chiffrés avec une clé `.key` elle-même chiffrée par une clé codée en dur | **Secrets dans le trousseau OS** (crate `keyring`) |
| Config `default.config.ini` par section (`[db.postgres]`, `[keybindings.queryEditor]`…) | Config TOML équivalente ; raccourcis = *actions* + *key contexts* GPUI |

## 3. Stack 100 % Rust

| Besoin | Choix | Notes |
|---|---|---|
| Framework UI | **GPUI** (framework de Zed), via `gpui-pre =0.3.9` + `gpui-pre-platform` | Pré-1.0, API mouvante : **versions épinglées avec `=`** |
| Composants | **gpui-component 0.7.3** (Longbridge, Apache-2.0) | 75 modules : `table` (DataTable virtualisée), `input` (Editor + `CompletionProvider`, `HoverProvider`), `dock`/`tab`, `tree`, `menu`, `command`, `notification`, `dialog`, `sidebar`, `status_bar`, `resizable`, `theme`… |
| Coloration SQL | tree-sitter via `highlighter` de gpui-component + grammaire SQL | |
| Runtime async | **tokio** (drivers) relié à l'exécuteur GPUI | Voir §4 |
| Drivers | `sqlx`, `tokio-postgres`, `tiberius`, `duckdb`, `clickhouse`, `mongodb`… | §7 |
| SSH | `russh` | bastion, agent, `~/.ssh/config` |
| Parsing SQL | lexer tolérant maison + `sqlparser-rs` | découpage, paramètres, classification lecture/écriture |
| Stockage | `rusqlite` ou `sqlx` (SQLite) + `keyring` | |
| MCP | `rmcp` (SDK MCP officiel en Rust) | sous-commande `ruche mcp` |
| Packaging | `cargo-packager` (.app/.dmg, .msi, AppImage/deb) | signature + notarisation macOS |

Ces versions sont celles utilisées en octobre 2026 par **DBFlux**, un client de bases de données GPUI open source
(voir §11) : c'est une combinaison qui fonctionne en production aujourd'hui.

## 4. Architecture

```
┌──────────────────────────── binaire `ruche` ─────────────────────────────┐
│ UI GPUI (thread principal)                                               │
│   ruche-app : Workspace · DockArea · onglets · actions & keymap          │
│   ruche-ui  : SqlEditor · ResultGrid éditable · ConnectionForm · thèmes  │
│        │ cx.spawn(...)  ▲ entity.update(cx, …)                           │
│        ▼                │                                                │
│   Bridge : CoreHandle (Global GPUI) → tâche tokio → oneshot → retour UI  │
├──────────────────────────────────────────────────────────────────────────┤
│ Runtime tokio (pool de threads dédié)                                    │
│   ruche-session : SessionManager · QueryRegistry · transactions · cache  │
│   ruche-core    : traits Driver/Connection · Value · ResultSet · dialectes│
│   ruche-drivers : pg · mysql · sqlite · mssql · duckdb · clickhouse · …  │
│   ruche-ssh · ruche-store (app.db + keyring) · ruche-sql (lexer, complétion)│
└──────────────────────────────────────────────────────────────────────────┘
        `ruche mcp`  → même binaire, sans UI, serveur MCP stdio (ruche-mcp)
```

**Point technique clé : deux exécuteurs.** GPUI a son propre exécuteur (thread principal + pool) ;
les drivers exigent tokio. On démarre un runtime tokio au lancement, stocké dans un `Global` GPUI
(`CoreHandle`). Une vue appelle `core.run(fut)`, qui lance la future sur tokio et renvoie un
`oneshot::Receiver` attendu dans `cx.spawn`. Le résultat revient à l'entité via `this.update(cx, …)`.
Aucun appel bloquant sur le thread UI.

**Modèle d'état GPUI** : `Entity<Workspace>` → `Entity<ConnectionSession>` → `Entity<QueryTab>` /
`Entity<TableTab>` / `Entity<StructureTab>`. Chaque onglet est un panneau du `DockArea` (sérialisable : restauration
des onglets au redémarrage).

Workspace Cargo :

```
ruche/
  crates/
    ruche-core/      # types, traits, capacités, dialectes SQL (quoting, DDL, aperçu ALTER)
    ruche-drivers/   # une feature Cargo par moteur
    ruche-session/   # sessions, registre de requêtes, annulation, transactions
    ruche-sql/       # lexer tolérant, découpage, paramètres, moteur d'autocomplétion (sans UI, testable)
    ruche-ssh/       # tunnels
    ruche-store/     # app.db + migrations + keyring
    ruche-mcp/       # serveur MCP
    ruche-ui/        # composants réutilisables au-dessus de gpui-component
    ruche-app/       # workspace, écrans, actions, keymap
  apps/ruche/        # main.rs : GUI par défaut, sous-commande `mcp`
```

`ruche-ui` sert aussi de **pare-chocs** : les écrans n'utilisent jamais gpui-component directement, ce qui limite
l'impact des changements d'API de GPUI.

## 5. Contrat du cœur

```rust
#[async_trait]
pub trait Connection: Send + Sync {
    fn capabilities(&self) -> &Capabilities;          // ≈ conn/supportedFeatures
    async fn version(&self) -> Result<String>;
    async fn list_databases(&self) -> Result<Vec<String>>;
    async fn list_schemas(&self) -> Result<Vec<String>>;
    async fn list_entities(&self, f: &EntityFilter) -> Result<Vec<Entity>>;     // tables, vues, vues mat., routines
    async fn describe_table(&self, t: &TableRef) -> Result<TableInfo>;        // colonnes, PK, FK in/out, index, triggers
    async fn select_page(&self, t: &TableRef, q: &PageQuery) -> Result<ResultSet>; // filtres, tri, pagination
    async fn count(&self, t: &TableRef, f: &[Filter]) -> Result<u64>;
    async fn execute(&self, sql: &str, p: &Params, c: CancellationToken) -> Result<Vec<ResultSet>>;
    async fn apply_changes(&self, cs: &ChangeSet) -> Result<ApplyReport>;     // insert/update/delete en 1 transaction
    async fn ddl(&self, obj: &ObjectRef) -> Result<String>;                   // create scripts
    fn dialect(&self) -> &dyn Dialect;                                         // génération SQL + aperçu
    async fn begin(&self) -> Result<()>; async fn commit(&self) -> Result<()>; async fn rollback(&self) -> Result<()>;
}
```

- `Value` sans perte : `Null | Bool | Int(i64) | BigInt(String) | Float | Decimal(String) | Text | Bytes | Json | Uuid | Date/Time/Timestamp(tz) | Array | Other(String)`.
- `ResultSet` stocké en colonnes dans un `Arc`, partagé tel quel avec la grille (pas de copie).
  Au-delà d'un seuil configurable (ex. 1 M de cellules), spill vers un fichier temporaire (Arrow IPC).
- Annulation native : PostgreSQL `cancel_token`, MySQL `KILL QUERY`, SQL Server *attention*, SQLite `interrupt`.

## 6. UX à reproduire, et avec quels composants

### Visuel
- Thèmes **dark / light / solarized / solarized-dark / system**, en fichiers JSON de thème gpui-component.
  Tokens relevés dans le thème sombre de Beekeeper : fond `#181818`, primaire jaune `#fad83b`, secondaire `#4ad0ff`,
  succès `#15db95`, avertissement `#ff8d21`, danger `#ff5d59`, texte en blanc à 87 / 67 / 57 / 37 % d'opacité.
- Polices embarquées dans le binaire : Roboto (UI), Source Code Pro (éditeur, grille).
  Icônes : Material Icons en SVG (Apache 2.0).
- Barre de titre personnalisée (`TitleBar` de gpui-component), couleur par connexion sur le bandeau et les onglets.

### Correspondance écrans → composants

| Écran / fonction | Composants | À construire nous-mêmes |
|---|---|---|
| Écran de connexion (liste, dossiers, épingles, formulaire par moteur, coller une URL, SSH, SSL, Tester) | `sidebar`, `list`, `form`, `input`, `select`, `tab`, `switch`, `dialog` | Formulaires par moteur, parseur d'URL |
| Barre latérale : bases → schémas → tables/vues/routines, filtre, épingles, masquer | `tree`, `searchable_list`, `menu` (clic droit) | Chargement paresseux de l'arbre |
| Onglets requête / table / structure, réordonnables, restauration, rouvrir le dernier fermé | `dock` (DockArea, onglets déplaçables, splits, sérialisable) | Historique des onglets fermés |
| Recherche rapide `Ctrl/Cmd+P` | `command` | Index flou (tables, requêtes sauvegardées, historique) |
| Éditeur SQL | `input::Editor` + tree-sitter | **Moteur d'autocomplétion** (`ruche-sql`) branché sur `CompletionProvider` ; type de colonne au survol via `HoverProvider` ; exécution instruction courante/sélection |
| Résultats multiples, temps, nb de lignes, Annuler | `tab`, `status_bar`, `spinner` | |
| Grille de résultats et de table | `table::DataTable` (virtualisée, colonnes fixes/redimensionnables, tri, sélection ligne/colonne/cellule) | **Édition en ligne** (voir ci-dessous), copier sous, coller |
| Filtres (colonne / opérateur / valeur, ET/OU) + WHERE brut, pagination | `form`, `select`, `input`, `pagination` | Constructeur de filtres |
| Panneau JSON, éditeur modal de texte long | `sheet`, `dialog`, `input::Editor` | |
| Structure (colonnes, index, relations, triggers) + aperçu ALTER | `table`, `dialog`, `diff` (aperçu avant/après) | Générateurs ALTER par dialecte |
| Notifications, confirmations | `notification`, `dialog` | |
| Diagramme ER | `canvas` GPUI | Disposition du graphe (crate `layout-rs` ou force-directed maison) |

### Édition en ligne (le morceau le plus délicat côté UI)
`DataTable` gère sélection et virtualisation mais pas l'édition : on la construit.
- Double-clic ou `Entrée` sur une cellule : un `Input` est superposé aux bornes de la cellule
  (`TableVisibleRange` + mesure des colonnes). `Entrée` valide, `Échap` annule, `Tab` passe à la cellule suivante.
- Chaque modification alimente un `ChangeSet` (insert / update / delete) avec historique : undo/redo.
- Rendu des états dans `TableDelegate` : ligne ajoutée en vert, cellule modifiée en jaune, ligne supprimée en rouge.
- `Ctrl/Cmd+S` : aperçu du SQL généré dans un `dialog`, puis `apply_changes` en une transaction.
- Clic sur une valeur de FK : ouvre la ligne référencée dans un nouvel onglet table filtré.

### Autocomplétion SQL (`ruche-sql`, sans UI, testée unitairement)
- Lexer tolérant (le SQL est incomplet pendant la frappe, `sqlparser-rs` échouerait) + détection de contexte :
  après `FROM`/`JOIN` → tables ; après `alias.` → colonnes de la table aliasée ; liste du `SELECT`/`WHERE` →
  colonnes des tables présentes ; sinon mots-clés et fonctions du dialecte.
- Cache de schéma par session, invalidé après un DDL ou un rafraîchissement.

### Raccourcis
Les raccourcis Beekeeper deviennent des **actions GPUI** liées à des *key contexts* (`QueryEditor`, `ResultTable`,
`TableTable`, `Tab`…), ce qui reproduit naturellement les sections `[keybindings.*]` de sa config. Exemples à
reprendre : `Ctrl/Cmd+Entrée` / `F5` (exécuter la sélection ou l'instruction courante), `Ctrl/Cmd+Shift+Entrée` (tout),
`Ctrl/Cmd+I` (résultat vers fichier), `Ctrl/Cmd+L` (focus éditeur), ``Ctrl/Cmd+` `` (bascule éditeur ↔ résultats),
`Shift+↑/↓` (résultat précédent/suivant), `Ctrl/Cmd+F` (filtre), `Ctrl/Cmd+S` (appliquer),
`Ctrl/Cmd+Shift+C` / `R` (commit / rollback). Fichier utilisateur `keymap.toml` pour les surcharger.

Mode vim : pas de composant prêt ; à reporter en phase de finitions (implémentation d'un sous-ensemble).

## 7. Drivers

| Priorité | Moteurs | Crate Rust |
|---|---|---|
| P1 | PostgreSQL, CockroachDB, Redshift | `sqlx` (postgres) / `tokio-postgres` pour l'annulation |
| P1 | MySQL, MariaDB, TiDB | `sqlx` (mysql) |
| P1 | SQLite | `rusqlite` (extensions chargeables, `interrupt`) |
| P2 | SQL Server | `tiberius` |
| P2 | DuckDB | `duckdb` |
| P2 | ClickHouse | `clickhouse` |
| P2 | libSQL / Turso | `libsql` |
| P3 | MongoDB | `mongodb` (UI document dédiée) |
| P3 | Redis | `redis` (UI clé/valeur dédiée) |
| P3 | Cassandra / ScyllaDB | `scylla` |
| P3 | BigQuery, DynamoDB | `gcp-bigquery-client`, `aws-sdk-dynamodb` |
| P4 | Oracle, Firebird, Trino, SurrealDB | `oracle` (Instant Client), `rsfbclient`, `prusto`, `surrealdb` |

L'essentiel de l'effort par driver est dans les **requêtes de métadonnées** et la génération DDL/ALTER par dialecte.

## 8. Le serveur MCP local

Même binaire, **sous-commande `ruche mcp`** (crate `rmcp`, transport stdio). Elle réutilise `ruche-core`,
`ruche-drivers`, `ruche-ssh` et `ruche-store`, sans charger GPUI. Elle fonctionne même quand l'app est fermée.

```bash
claude mcp add ruche -- /Applications/Ruche.app/Contents/MacOS/ruche mcp
```

### Outils exposés
| Outil | Rôle |
|---|---|
| `list_connections` | id, nom, type, étiquette d'environnement, mode MCP — **jamais de secrets** |
| `list_databases` / `list_schemas` / `list_tables` | navigation |
| `describe_table` | colonnes, types, PK, FK, index, estimation du nombre de lignes |
| `search_schema` | recherche floue de tables/colonnes **sur toutes les connexions** |
| `run_query` | SQL + paramètres, `max_rows` (déf. 200), timeout ; lecture seule par défaut |
| `explain_query` | plan d'exécution |
| `sample_rows` | N lignes d'une table |
| `list_saved_queries` | requêtes favorites de l'utilisateur |

Ressources MCP : `ruche://{connexion}/{schéma}/{table}` (schéma en texte).

### Sécurité (plusieurs couches)
1. **Opt-in par connexion** : `désactivé` (défaut) / `lecture seule` / `lecture-écriture`. Une connexion
   étiquetée *production* est plafonnée à la lecture seule.
2. **Lecture seule imposée par la base**, pas seulement par l'analyse du SQL : PostgreSQL
   `default_transaction_read_only = on`, MySQL `START TRANSACTION READ ONLY`, SQLite ouverture `mode=ro`.
   SQL Server n'a pas d'équivalent fiable : l'UI demande un login en lecture seule.
3. **Classification `sqlparser-rs`** en défense supplémentaire (refus de DDL/DML en lecture seule).
4. **Limites** : lignes, octets, `statement_timeout`.
5. **Écritures** (si autorisées) : si l'app est ouverte, confirmation dans une fenêtre native GPUI avec aperçu des
   instructions ; sinon, refus.
6. **Journal d'audit** dans app.db (connexion, SQL, durée, nb de lignes), consultable dans l'app.
7. Les secrets restent dans le trousseau ; le process MCP les lit lui-même.
   L'app rappelle que les données renvoyées à Claude quittent la machine.

## 9. Stockage et sécurité de l'app

- `app.db` SQLite + migrations, tables équivalentes à celles de Beekeeper : connexions, dossiers, épingles,
  disposition du dock et onglets, historique, favoris, réglages, préréglages de formatage, audit MCP.
- Secrets : `keyring` (Trousseau macOS, Credential Manager Windows, Secret Service Linux).
- Plus de webview : la surface d'attaque Electron (remote, fuses, preload non sandboxé) disparaît.
- **Importeur Beekeeper** (optionnel) : reprendre les connexions de l'`app.db` Beekeeper existante de l'utilisateur.

## 10. Phases

Estimations pour un développeur à temps plein. Le natif coûte environ 30 % de plus que la version Tauri,
surtout sur la grille éditable, l'autocomplétion et les formulaires. À réévaluer après la phase 0.

| Phase | Contenu | Durée | Terminé quand… |
|---|---|---|---|
| **0. Fondations + spikes** | Workspace, fenêtre GPUI + gpui-component épinglés, thème sombre Beekeeper-like, bridge tokio ↔ GPUI, types `Value`/`ResultSet`, app.db + keyring, CI macOS/Windows/Linux. **Spikes** : édition en ligne sur `DataTable` ; `CompletionProvider` branché sur un faux schéma ; DataTable sur 1 M de lignes ; **évaluation de DBFlux** (§11) | 2–3 sem. | Les 3 spikes passent ; décision « partir de zéro / s'appuyer sur DBFlux » prise |
| **1. MVP lecture** | Drivers PG/MySQL/SQLite, écran de connexion (URL, test, couleurs, dossiers, TLS), arbre latéral, dock + onglets requête, éditeur avec coloration + autocomplétion schéma/alias, exécution courante/sélection/script, annulation, multi-résultats, grille en lecture, historique, restauration des onglets | 5–7 sem. | Remplace Beekeeper pour les requêtes du quotidien sur ces 3 moteurs |
| **2. MCP** | `ruche mcp`, outils de lecture, modes par connexion, lecture seule imposée, audit + écran de réglages | 1–2 sem. | Claude Code interroge toutes les bases activées, aucun secret exposé |
| **3. Édition + SSH** | Onglets table, filtres, tri, pagination, édition en ligne + appliquer avec aperçu SQL, undo/redo, navigation FK, panneau JSON, copier sous, export ; tunnels SSH | 5–6 sem. | Parcours « filtrer → éditer → appliquer » via un tunnel SSH |
| **4. Structure** | Onglet structure, création/modification avec aperçu ALTER, DDL, création de base, tronquer/supprimer | 3 sem. | Ajout d'une colonne + d'un index sans écrire de SQL |
| **5. Drivers étendus** | P2 puis P3/P4 (UI dédiées Mongo/Redis) | continu | Un moteur = métadonnées + édition + tests d'intégration |
| **6. Finitions** | Recherche rapide, `keymap.toml`, formateur, thèmes light/solarized, diagramme ER, import CSV/XLSX, transactions manuelles, sous-ensemble vim, packaging signé/notarisé, mises à jour auto, importeur Beekeeper | 4 sem. | Version 1.0 distribuable |

Total indicatif : **6 à 7 mois** jusqu'à la 1.0 pour une personne.

Tests :
- `ruche-sql`, `ruche-core` : tests unitaires (complétion, découpage, génération SQL).
- Drivers : `testcontainers` (une base Docker par moteur) pour les métadonnées et `apply_changes`.
- UI : `#[gpui::test]` / `TestAppContext` pour les parcours clés (édition en ligne, onglets, raccourcis).

## 11. DBFlux : un projet très proche à évaluer en phase 0

La recherche a fait ressortir **DBFlux** (github.com/0xErwin1/dbflux) : client de bases de données en
**Rust + GPUI + gpui-component 0.7.3**, sous licence **Apache-2.0/MIT**, avec ~14 drivers (PostgreSQL, MySQL,
SQLite, SQL Server, MongoDB, Redis, DynamoDB, ClickHouse, DuckDB…), SSH, éditeur multi-onglets, restauration de
session, et **un serveur MCP intégré** (`dbflux mcp`) avec classification des opérations et approbation humaine des
écritures. Environ 480 étoiles en octobre 2026.

Ce qu'il ne couvre pas d'après sa documentation : autocomplétion explicite dans l'éditeur SQL général (mentionnée
seulement pour le constructeur de requêtes et le filtre WHERE), et un ressenti « Beekeeper » (il se présente comme
une plateforme orientée clavier).

**Mise à jour du 17/10/2026** : l'évaluation est faite, voir [evaluation-dbflux.md](evaluation-dbflux.md).
Recommandation : fork proche de l'amont (option B), environ 8 à 11 semaines au lieu de 6 à 7 mois.
L'autocomplétion de DBFlux est en fait très complète (tree-sitter, alias, CTE).

Trois options, à trancher à la fin de la phase 0 :
- **A. Partir de zéro** selon ce plan, en s'inspirant librement de son code (la licence le permet).
- **B. Forker DBFlux** et le faire converger vers l'UX Beekeeper : gain de plusieurs mois sur drivers, SSH et MCP,
  mais dette de divergence avec l'amont et architecture héritée.
- **C. Contribuer à DBFlux** (autocomplétion, édition en ligne, thème) : effort minimal, mais on ne maîtrise pas
  la direction produit.

## 12. Risques et points ouverts

- **GPUI pré-1.0** : changements d'API fréquents. Parade : versions épinglées, couche `ruche-ui`, mises à jour
  groupées et volontaires.
- **Grille éditable** à construire au-dessus de `DataTable` : c'est le premier spike de la phase 0.
- **Accessibilité** (lecteurs d'écran) et saisie IME : plus faibles en GPUI que dans un navigateur. À tester tôt
  sur les trois OS.
- **Windows / Linux** : supportés par GPUI mais moins éprouvés que macOS ; CI sur les trois OS dès la phase 0.
- **Volume du travail sur les métadonnées** : c'est le vrai coût par moteur.
- **Licence** : le dépôt public de Beekeeper Studio est sous GPLv3 et son dossier `src-commercial` sous licence
  commerciale (à vérifier). Approche recommandée : **réimplémentation propre**, en se servant de l'analyse comme
  spécification fonctionnelle, sans copier code, CSS, nom, logo ni icônes propres à Beekeeper.
  À faire valider si le projet doit être distribué ou commercialisé.
