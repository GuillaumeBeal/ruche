# Plan de démarrage du fork DBFlux → Ruche

> Décision du 17/10/2026 : option B, fork de DBFlux qui reste proche de l'amont
> (voir [evaluation-dbflux.md](evaluation-dbflux.md)). « Ruche » est un nom de travail, à valider (§8).

## 1. Principe directeur : deux files de changements

| File | Contenu | Destination | Règle |
|---|---|---|---|
| **Amont** | Corrections et fonctionnalités utiles à tous : précision DECIMAL, enregistrement transactionnel, SSH côté MCP, navigation FK… | PR vers `0xErwin1/dbflux` | Une issue approuvée avant chaque PR, ≤ 400 lignes par PR |
| **Fork** | Ce que l'amont ne voudra pas : nom, logo, chemins de données, thème Beekeeper, coins droits | Branche `ruche` du fork | Surface limitée à une liste de fichiers déclarée (§3) |

**Objectif mesurable** : la différence fork ↔ amont reste sous ~30 fichiers et la synchronisation hebdomadaire prend
moins d'une heure. Au-delà, on réévalue (§9).

Ce que l'amont a déjà signalé :
- Le mainteneur est réactif et suit un processus clair : on réserve un sujet en commentant
  [#773](https://github.com/0xErwin1/dbflux/issues/773), on en discute, l'issue reçoit le label `status:approved`,
  puis on ouvre la PR.
- Il est **protecteur du design** : l'éditeur de couleurs syntaxiques de [#986](https://github.com/0xErwin1/dbflux/issues/986)
  a été écarté « le temps de le confronter au design de l'app ». Le thème Beekeeper restera donc dans le fork.
- Les PR sont fusionnées en *squash*, doivent passer `tests.yml` + `style.yml`, et toute modification de `docs/`
  inclut ses traductions (es, ko, zh_Hans).

## 2. Mise en place (semaine 1, jours 1–2)

### Prérequis sur ce Mac (à faire vous-même, ce sont des réglages système)
La compilation GPUI a besoin du compilateur Metal de Xcode :
```bash
sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
```
```bash
sudo xcodebuild -license accept
```
Sinon, on garde le contournement `runtime_shaders` utilisé pendant l'évaluation, mais ce serait un écart de plus
avec l'amont.

### Dépôts et branches
- Fork GitHub sous votre compte, dossier local `~/GitHub/ruche`.
- Remotes : `origin` = votre fork, `upstream` = `0xErwin1/dbflux`.
- Branches :

| Branche | Rôle | On y commite ? |
|---|---|---|
| `main` | Miroir exact de `upstream/main` | Jamais (fast-forward uniquement) |
| `ruche` | Produit : `main` + changements fork | Oui, via PR depuis `fork/*` |
| `up/<sujet>` | Une contribution amont, partie de `upstream/main` | Oui → PR vers l'amont |
| `fork/<sujet>` | Un changement propre au fork, partie de `ruche` | Oui → PR vers `ruche` |

- Synchronisation : chaque semaine, `main` en fast-forward, puis **merge** de `main` dans `ruche` (pas de rebase :
  la branche est publiée et le merge garde l'historique des résolutions de conflits).
- Quand une de nos PR est fusionnée en amont, la version fork correspondante est supprimée de `ruche` au merge suivant.

### CI du fork
- Garder `tests.yml` et `style.yml` (y compris les tests Docker des drivers).
- Désactiver dans le fork les workflows de publication amont (release, nightly, installer, web) : premier
  changement de la liste fork.
- Ajouter un job `fork-surface` qui échoue si `git diff upstream/main...ruche --name-only` sort de la liste déclarée
  dans `FORK_SURFACE.txt`.

## 3. Changements propres au fork

### 3.1 Renommage minimal (semaine 1)
La politique de marque autorise à garder « DBFlux » dans le code, l'historique et la doc. Seuls le nom affiché, le
logo et les identifiants d'une build modifiée doivent changer. Les points concernés sont déjà centralisés :

| Élément | Fichier | Changement |
|---|---|---|
| Nom affiché, identifiant d'app (bundle id, `WM_CLASS`), nom du fichier SQLite | `crates/dbflux_core/src/release_channel.rs` (`display_name`, `app_id`, `db_file_name`) | « Ruche », `ruche`, `ruche.db` |
| Dossier de données (`~/.local/share/dbflux`) | `crates/dbflux_storage/src/paths.rs` (`data_dir`) | `ruche` → coexistence possible avec DBFlux |
| Service du trousseau | `crates/dbflux_core/src/storage/secrets.rs` (`SERVICE_NAME`) | `ruche` (secrets séparés) |
| Icônes et logo | `packaging/icons/`, `resources/branding/` | Nouveaux fichiers |
| Textes de l'interface | `crates/dbflux_i18n/locales/*.yml` (27 occurrences × 5 langues) | Voir ci-dessous |
| Nom du serveur MCP et de son client | `crates/dbflux_mcp_server` (24 littéraux) | « Ruche » |

Pour les traductions, mieux vaut proposer d'abord **en amont** de remplacer « DBFlux » écrit en dur par la
variable `%{app}`, déjà utilisée dans les menus. C'est un changement neutre, utile aux distributions nightly.
S'il est accepté, le renommage des textes ne coûte plus rien au fork.

### 3.2 Look Beekeeper (semaines 2–3)
| Élément | Approche à diff minimal |
|---|---|
| Palettes | Nouveau fichier `crates/dbflux_components/src/theme_ruche.rs` (sombre : fond `#181818`, primaire `#fad83b`, secondaire `#4ad0ff`, succès `#15db95`, avertissement `#ff8d21`, danger `#ff5d59` ; plus une palette claire), branché par une feature Cargo `ruche` dans `theme.rs` (quelques lignes) |
| Coins | Tous les chanfreins passent par `Chamfer::paint` (`primitives/chamfer.rs`). Sous la feature `ruche`, profondeur forcée à 0 (coins droits). Coins arrondis en option plus tard |
| Couleurs syntaxiques | `SyntaxColors` dans `tokens.rs`, sous la même feature |
| Polices | Roboto + Source Code Pro ajoutées aux polices embarquées (`typography.rs`), choisies par défaut via les réglages existants |
| Densité, raccourcis | Valeurs par défaut uniquement (réglages existants) : aucune modification de code |

Le design de DBFlux interdit les couleurs codées en dur hors `tokens.rs` (tests `style_guardrails.rs`) : c'est
un atout, car le thème reste confiné à 3–4 fichiers.

## 4. Contributions amont, dans l'ordre

Chaque contribution suit le même circuit : issue (modèle du dépôt) → approbation → branche `up/*` → PR.
L'ordre commence par des sujets petits et consensuels, pour gagner la confiance du mainteneur avant les sujets
plus lourds.

| # | Sujet | Issue | Taille | Pourquoi maintenant |
|---|---|---|---|---|
| 1 | **Précision des DECIMAL MySQL** : vérifier avec un test Docker, puis corriger vers `Value::Decimal` | À créer (bug) | S | Petit bug net : bon premier contact |
| 2 | **`%{app}` dans les traductions** au lieu de « DBFlux » en dur | À créer | S | Réduit le coût du renommage pour tous |
| 3 | **MCP : tunnels SSH et proxys** dans le gestionnaire d'accès du serveur MCP | À créer | M | Indispensable pour « toutes les BDD configurées » |
| 4 | **MCP : requête SQL libre en lecture seule** (lecture seule imposée par la base, timeout, limite de lignes) + timeout sur `execute_script` | À créer ; vérifier d'abord ce que [#844](https://github.com/0xErwin1/dbflux/issues/844) a déjà couvert | M | Le cœur de l'usage avec Claude |
| 5 | **MCP : enregistrer SQL Server** dans le serveur autonome | À créer | S | Couverture des bases |
| 6 | **Navigation FK dans la grille** | [#1087](https://github.com/0xErwin1/dbflux/issues/1087), déjà `status:approved`, non attribuée | L (3 PR : `fetch_rows_by_pks`, inspecteur cliquable, libellés dans la grille) | Demandée et approuvée : à réserver vite sur #773 |
| 7 | **Enregistrement transactionnel** des modifications de la grille + aperçu SQL de l'ensemble | À créer (discussion de conception d'abord : `ExecutionSession`) | M | Sécurité des données, attente forte d'un utilisateur Beekeeper |
| 8 | **Onglets** : glisser-déposer, restauration des onglets de table | À créer | M | Ressenti Beekeeper |
| 9 | **Copier en CSV / JSON / Markdown** | À créer | S | Ressenti Beekeeper |
| 10 | **Résultats de requête libre éditables** quand ils sont sûrs (une table, clé primaire) | Suite de [#841](https://github.com/0xErwin1/dbflux/issues/841) (le mainteneur l'a jugé « plus gros ») | L | Seulement après plusieurs PR acceptées |

Si une contribution est refusée en amont, elle bascule dans la file fork, en gardant le même découpage.

## 5. MCP utilisable dès la semaine 1

Sans attendre les contributions, le serveur MCP de DBFlux fonctionne déjà pour les connexions **directes** (sans SSH) :
1. Dans l'app : activer MCP sur les connexions voulues, créer le client de confiance `claude-code`, lui assigner la
   politique « lecture seule ».
2. L'enregistrer dans Claude Code :
   ```bash
   claude mcp add ruche -- /Applications/Ruche.app/Contents/MacOS/ruche mcp --client-id claude-code
   ```
3. Limites actuelles (corrigées par les contributions 3 à 5) : pas de SSH, pas de SQL Server, SQL libre seulement
   via `create_script` + `execute_script`.

## 6. Calendrier des 4 premières semaines

| Semaine | Fork | Amont | Résultat attendu |
|---|---|---|---|
| **1** | Fork, branches, CI allégée + `fork-surface`, renommage minimal, première build Ruche, MCP branché sur Claude Code | Message de présentation sur #773 / Discussions (plan et liste des contributions prévues) ; réservation de #1087 ; issue DECIMAL avec test de reproduction | Ruche démarre à côté de DBFlux sans conflit de données ; Claude interroge une base directe |
| **2** | Palettes Ruche + coins droits | PR DECIMAL ; issue `%{app}` ; issue MCP SSH | Ruche ressemble à Beekeeper en sombre |
| **3** | Polices, couleurs syntaxiques, palette claire | PR `%{app}` ; PR MCP SSH (si approuvée) ; début de #1087 (PR 1/3) | Thème complet ; MCP sur bases SSH (dans le fork au minimum) |
| **4** | Première synchronisation « réelle » mesurée (durée, conflits) | #1087 PR 2/3 ; discussion de conception « enregistrement transactionnel » | Point d'étape §9 |

Semaines 5 à 11 : contributions 4 à 9, au rythme des approbations ; ce qui bloque en amont passe dans le fork.

## 7. Hygiène du fork

- `FORK_SURFACE.txt` : liste des fichiers que le fork a le droit de modifier, vérifiée en CI.
- Chaque commit fork préfixé `fork(scope): …` ; chaque commit amont en Conventional Commits, comme demandé.
- Avant chaque PR amont : `python3 scripts/lint.py fmt --check`, `python3 scripts/lint.py clippy`,
  `cargo test --workspace` (ou `cargo nextest run --workspace`).
- Journal de synchro (`SYNC_LOG.md`) : date, commit amont, durée, conflits. C'est la donnée qui décide du §9.

## 8. Décisions à prendre de votre côté

1. **Nom définitif** (Ruche ou autre) : vérifier la disponibilité (nom de domaine, crates.io, marques déposées).
2. **Compte GitHub du fork** : votre compte personnel ou une organisation.
3. **Distribution** : usage personnel uniquement, ou builds publiées. Si publiées, prévoir la signature et la
   notarisation macOS (certificat Apple Developer).
4. **Visibilité de votre démarche** auprès du mainteneur : annoncer le fork ouvertement est recommandé ; ça
   facilite l'acceptation des PR.

## 9. Points de contrôle et critères de sortie

Au bout de 4 semaines, puis tous les mois :

| Signal | Seuil d'alerte | Réaction |
|---|---|---|
| Fichiers modifiés par le fork | > 30 | Revoir la conception des changements fork (features Cargo, fichiers séparés) |
| Durée de synchronisation hebdo | > 1 h en moyenne | Passer à une synchro toutes les 2 semaines, ou bien réduire la surface |
| PR amont refusées sur les sujets clés (3, 4, 7) | 2 refus | Les porter dans le fork ; si la surface explose, réévaluer : fork dur, ou retour au plan « Ruche from scratch » en réutilisant le moteur d'autocomplétion de DBFlux |
| Activité amont | Plus de commits pendant 2 mois | Le fork devient de fait autonome : prévoir de reprendre la maintenance du vendoring GPUI |
