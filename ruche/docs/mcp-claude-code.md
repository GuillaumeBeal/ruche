# Brancher le serveur MCP de Ruche sur Claude Code

Le serveur MCP est la sous-commande `mcp` du binaire Ruche. Il lit la même base de réglages que l'app
(connexions, clients de confiance, politiques, audit) et tourne même quand l'app est fermée.

## 1. Construire l'app

```bash
ruche/scripts/bundle-macos.sh
```

Le binaire se trouve ensuite dans `target/Ruche.app/Contents/MacOS/dbflux`. Copier `Ruche.app` dans
`/Applications` donne un chemin stable pour la configuration MCP.

## 2. Configurer la gouvernance dans l'app (une fois)

1. **Réglages → MCP → Clients** : ajouter un client de confiance, identifiant `claude-code`, actif.
2. **Gestionnaire de connexions → onglet MCP**, pour chaque connexion à exposer :
   activer MCP, choisir l'acteur `claude-code` et le rôle **Read Only**.
3. Redémarrer le serveur MCP après chaque changement de gouvernance : il relit les réglages au démarrage.

Sans client de confiance, le serveur refuse de démarrer ; c'est voulu.

## 3. Déclarer le serveur dans Claude Code

```bash
claude mcp add ruche -- /Applications/Ruche.app/Contents/MacOS/dbflux mcp --client-id claude-code
```

## 4. Ce qui fonctionne aujourd'hui, et ce qui manque

| Fonctionne | Limite actuelle | Correction prévue |
|---|---|---|
| Connexions directes (PostgreSQL, MySQL/MariaDB, SQLite, ClickHouse, MongoDB, Redis, DynamoDB, Turso, DuckDB, Redshift) | Connexions via **tunnel SSH ou proxy** refusées | Contribution amont n° 3 |
| Lecture structurée (`select_data`, `count_records`, `aggregate_data`), schéma, EXPLAIN | **SQL libre** seulement via `create_script` + `execute_script` | Contribution amont n° 4 |
| Lecture seule imposée par la base (PG, MySQL, SQLite, ClickHouse) | **SQL Server** non disponible côté MCP | Contribution amont n° 5 |
| Approbations dans l'app, audit | Pas de timeout sur `execute_script` | Contribution amont n° 4 |

Pour une connexion en lecture seule, le vrai garde-fou reste un **compte de base de données en lecture seule**
(voir `docs/MCP_AI_INTEGRATION.md`, section sur l'application de la lecture seule).
