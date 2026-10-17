# Évaluation de DBFlux comme base pour Ruche

Date : 17 octobre 2026 · Version évaluée : `main` @ `f8347d9` (v0.8.12 + 1 commit)
Méthode : clone, compilation release sur macOS, lecture du code par domaine, vérification ponctuelle des points
décisifs, test du serveur MCP en stdio. Le test visuel de l'interface n'a pas été fait (accès écran refusé).

## Verdict

**Recommandation : forker DBFlux, en gardant le fork aussi proche que possible de l'amont.**
On gagne d'emblée ce qui coûte le plus cher dans le plan initial (drivers, autocomplétion, SSH, MCP gouverné,
tests sur 13 moteurs), soit environ 4 mois sur 6 à 7. Les écarts avec l'UX Beekeeper sont réels mais localisés
(thème, grille, onglets, outils MCP). Les améliorations d'intérêt général seraient proposées en amont pour ne
pas porter seuls une base de 850 000 lignes.

Le risque principal n'est pas technique : c'est la dépendance à un projet de 10 mois porté à ~90 % par une seule
personne, qui publie une version tous les 2-3 jours.

## Fiche d'identité

| | |
|---|---|
| Licence | MIT ou Apache-2.0 ; nom et logo DBFlux réservés (`TRADEMARK.md`) → renommage obligatoire |
| Activité | créé le 30/12/2025, 476 étoiles, 33 forks, versions 0.8.6 → 0.8.12 entre le 3 et le 16 octobre |
| Contributeurs | 0xErwin1 ≈ 1 400 commits ; les suivants : 45, 21, 14, 13 |
| Taille | **854 000 lignes de Rust** dans `crates/` (1,2 M avec les forks GPUI vendorisés), 49 crates |
| Stack | `gpui-pre 0.3.9` + `gpui-component 0.7.3`, **patchés et vendorisés** (~15 correctifs locaux) |
| Tests | ~5 400 tests UI, CI avec Docker pour 13 moteurs, tests MCP de bout en bout |
| Compilation | 2 min 40 s en release sur ce Mac, binaire de 156 Mo |

**Note de compilation** : la compilation échoue tant que la licence Xcode n'est pas acceptée (`sudo xcodebuild
-license`) ou que GPUI n'utilise pas l'option `runtime_shaders`. Je l'ai activée localement dans la copie
d'évaluation pour pouvoir compiler.

## Ce qui correspond à nos besoins

| Besoin du plan | État dans DBFlux | Preuve |
|---|---|---|
| Autocomplétion SQL contextuelle | **Très bon** : moteur tree-sitter (tables / colonnes selon la clause, alias, CTE, sous-requêtes, tolérant aux erreurs de frappe), tests dédiés | `dbflux_core/src/query/sql_context.rs`, `dbflux_ui_document/src/code/completion.rs` (3 200 lignes) |
| Éditeur | Instruction courante / sélection, marqueurs ▶ par instruction, annulation, un onglet de résultat par instruction, EXPLAIN, mode vim | `code/statements.rs`, `code/execution.rs` |
| Explorateur | Arbre bases → schémas → tables / colonnes / index / FK / routines, 3 dispositions | `dbflux_ui_sidebar` |
| Palette de commandes, raccourcis | Oui ; raccourcis par contexte, modifiables dans les réglages | `command_palette.rs`, `keymap/defaults.rs` |
| Édition en ligne | Partielle (voir écarts) : overlay de saisie, couleurs ajout / modif / suppression, undo/redo | `data_table/table.rs`, `model.rs` |
| Drivers | 14 moteurs ; PG, MySQL, SQLite, SQL Server complets (métadonnées, édition, annulation, DDL) | `docs/DRIVERS.md` |
| Secrets | Trousseau OS (`keyring`), références AWS Secrets Manager / SSM | `dbflux_core/src/storage/secrets.rs` |
| Connectivité | SSH (agent, clé, `ssh_config`), SOCKS5 / HTTP, AWS SSM | `dbflux_ssh`, `dbflux_proxy`, `dbflux_ssm` |
| MCP | `dbflux mcp`, rmcp 2.0 stdio, 39 outils, moteur de politiques (autoriser / demander / refuser), file d'approbation, audit, lecture seule **imposée par la base** pour les scripts | `dbflux_mcp*`, `dbflux_policy`, `dbflux_approval`, `dbflux_audit` |
| Couplage UI / drivers | Faible : les crates UI ne dépendent pas des drivers | `dbflux_app/src/app_state/bootstrap.rs` |

Constaté en direct : le serveur MCP refuse un `--client-id` inconnu et indique de déclarer le client comme
« trusted » d'abord.

## Écarts avec l'UX Beekeeper et notre plan

### Visuel (effort moyen)
- Identité forte : **coins chanfreinés à 45° sur tous les contrôles** (~74 fichiers) et accent violet.
- Thèmes Dark / Light / System **codés en dur** (`Palette::dark()` / `light()`), pas de fichiers de thème.
- Point positif : couleurs bien centralisées dans `dbflux_components/src/tokens.rs`, tests qui interdisent les
  couleurs en dur ailleurs. Ajouter une palette « Beekeeper » est simple ; retirer les chanfreins demande un
  réglage de style global.

### Grille et édition (le plus gros écart)
- **Les résultats d'une requête tapée dans l'éditeur ne sont pas éditables**. Seuls le parcours d'une table avec
  clé primaire et les requêtes mono-table du constructeur le sont. *Vérifié* (`mutations.rs:478`).
- **« Tout enregistrer » n'est pas transactionnel** : suppressions, puis insertions, puis mises à jour, ligne par
  ligne. *Vérifié* (`mutations.rs:1590`, aucune transaction).
- Pas d'aperçu SQL de l'ensemble des changements avant application.
- Pas de clic pour suivre une clé étrangère (seulement un inspecteur de ligne).
- Copier = TSV uniquement (CSV / JSON passent par l'export).

### Onglets
- Pas de réorganisation par glisser-déposer (prévu dans le code mais non branché).
- Seuls les onglets de code sont restaurés au redémarrage, pas les onglets de table.

### Drivers
- Probable perte de précision : les `DECIMAL` MySQL sont classés comme flottants (`ColumnKind::Float`).
  *Non testé sur une vraie base.*
- Pas de catalogue de fonctions par dialecte dans l'autocomplétion.

### MCP (pour « toutes les BDD configurées »)
- **Tunnels SSH et proxys refusés par le serveur MCP**. *Vérifié* (`dbflux_mcp_server/src/server.rs:93`).
  Bloquant si vos bases sont derrière SSH.
- **SQL Server non enregistré** côté MCP (ni InfluxDB, CloudWatch, S3, drivers RPC).
- Pas d'outil « exécuter un SELECT libre » : `select_data` est structuré ; le SQL libre passe par
  `create_script` + `execute_script`, classé en écriture.
- Pas de timeout sur `execute_script`.
- Les approbations ne sont visibles que si l'app est ouverte (pas de notification système).
- Les connexions doivent être activées une par une pour MCP et assignées au client.

## Options

| Option | Délai jusqu'à un outil proche du but | Coût long terme | Commentaire |
|---|---|---|---|
| **A. Partir de zéro** (plan Ruche) | 6–7 mois | Faible (code maîtrisé, ~100 k lignes) | Peut reprendre des morceaux de DBFlux (licence permissive), en particulier le moteur d'autocomplétion |
| **B. Fork « proche de l'amont »** *(recommandé)* | ~2–3 mois | Moyen : synchronisations régulières | Écarts traités en PR amont quand c'est d'intérêt général ; seuls le nom, le logo et le thème restent propres au fork |
| **C. Fork dur** | ~2–3 mois | Élevé : 1,2 M de lignes à porter seul | À éviter |
| **D. Contribuer uniquement** | Semaines | Nul | On ne contrôle ni le visuel ni les priorités |

### Travail estimé pour l'option B

| Chantier | Où | Amont ? | Durée |
|---|---|---|---|
| Renommage, logo, identifiant d'app | branding | non | 2–3 j |
| Palette Beekeeper + réglage « coins droits » | `tokens.rs`, `chamfer.rs` | à proposer | 1–2 sem. |
| Enregistrement transactionnel + aperçu SQL des changements | `data_grid_panel/mutations.rs` | **oui** | 1–2 sem. |
| Édition des résultats de requête mono-table avec PK | `mutations.rs`, détection de la source | oui | 1–2 sem. |
| Suivi des FK au clic, copier en CSV / JSON / Markdown | `data_table/` | oui | 1 sem. |
| Glisser-déposer des onglets, restauration des onglets table | `tab_bar.rs`, `tab_manager.rs` | oui | 1 sem. |
| MCP : tunnels SSH, outil `run_query` en lecture seule avec timeout, SQL Server | `dbflux_mcp_server` | **oui** | 1–2 sem. |
| Correction DECIMAL MySQL | `dbflux_driver_mysql` | oui | 1–2 j |

Total : **8 à 11 semaines**, dont l'essentiel est proposable en amont.

## Prochaines étapes proposées
1. Tester DBFlux vous-même 30 minutes sur vos vraies bases (installeur macOS non signé, ou la build locale).
2. Ouvrir une discussion avec le mainteneur sur 3 sujets : thème personnalisable, enregistrement transactionnel,
   SSH côté MCP. Son accueil conditionne le choix entre B et C.
3. Pour le MCP : activer le serveur DBFlux tout de suite sur vos connexions directes (sans SSH).
