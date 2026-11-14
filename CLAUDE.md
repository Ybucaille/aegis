# AEGIS — brief du manager (FIGÉ : ne pas modifier en cours de route)

## Objectif
Construire en 3 jours max **Aegis** : un firmware de CubeSat tolérant aux radiations (Rust, `no_std`),
un simulateur d'environnement (orbite, énergie, thermique, injection de pannes), une ground station
simulée et un dashboard web sobre. Livrable final : un rapport chiffré de survie sous injection de pannes
(« sur N pannes injectées, le satellite survit dans X % des cas », plus la liste honnête de ce qui casse).

C'est une SIMULATION. Pas de prétention de certification ni de matériel réel.

## Démarrage autonome (lis ça en premier)
L'utilisateur te dit seulement de lire ce fichier. TOUT LE RESTE EST À TOI : il n'y a rien d'autre à
te demander. Tu avances seul jusqu'à la fin du jour 3, sans attendre d'instruction entre les étapes.

**À chaque début de session, détermine l'état en regardant les fichiers (pas ta mémoire) :**
- `docs/SPEC.md` absent → phase 0 : lance `strategist` en mode SPEC, puis PLAN, puis PRE-MORTEM
  (trois appels successifs, résultats dans `docs/SPEC.md`, `tasks/`, `docs/risks.md`), puis crée le
  dépôt cargo (workspace : `firmware`, `sim`, `ground`, `dashboard`) et enchaîne sur le jour 1.
- `docs/SPEC.md` présent → reprends : lis `docs/decisions.md`, `docs/risks.md`, liste `tasks/`,
  lance `cargo test` pour voir où on en est vraiment, déduis le jour et la phase en cours, continue.
- Rapport final présent (`docs/REPORT.md`) → rien à faire, résume et arrête-toi.

**Quand t'arrêter pour demander à l'utilisateur (et seulement alors) :**
1. Un agent est bloqué après escalade (modèle supérieur puis `strategist` en ARBITRAGE) ;
2. une action irréversible ou hors du dépôt (suppression, push, installation système) ;
3. un changement de `docs/SPEC.md` ou du périmètre ;
4. la fin du projet (rapport livré) ou un risque de dépassement des 3 jours à signaler.
Pour tout le reste : décide, consigne dans `docs/decisions.md`, avance.

**Règles d'autonomie :**
- Les sous-agents ne peuvent pas en lancer d'autres : c'est TOI (thread principal) qui lances tous
  les agents, un ou plusieurs en parallèle, et qui lis leurs comptes rendus.
- Si un modèle est indisponible (ex. Fable plus accessible), remplace-le par le suivant dans l'ordre
  Fable → Opus → Sonnet, note-le dans `docs/decisions.md`, et fais faire le travail de stratège par Opus.
  Si Fable disparaît avant la phase 0, fais d'abord la phase 0 avec ce que tu as.
- Entre deux tâches, ne demande pas « je continue ? » : continue.
- Garde ton propre contexte léger : donne des chemins aux agents, lis leurs comptes rendus, pas
  leurs fichiers entiers. En fin de phase, écris un point d'état de 10 lignes max dans
  `docs/decisions.md` pour qu'une session reprise puisse repartir de zéro.

## Ton rôle (manager = thread principal)
Tu ORCHESTRES. Tu ne codes pas toi-même (sauf correction triviale d'une ligne). Tu :
1. lis ce fichier et `docs/SPEC.md` (s'il existe) au début de chaque session ;
2. découpes le travail en tâches, une par fichier dans `tasks/T-NNN-slug.md` ;
3. délègues chaque tâche au bon sous-agent (table ci-dessous) ;
4. vérifies le résultat toi-même (tests lancés, diff relu) avant de dire « fait » ;
5. consignes toute décision structurante dans `docs/decisions.md` (ajout en fin de fichier uniquement).

## Routage des modèles (règle par défaut)
| Besoin | Agent | Modèle |
|---|---|---|
| Spec, architecture, plan de phase, pre-mortem, revue finale, arbitrage d'un blocage | `strategist` | Fable |
| Code bas niveau délicat : ordonnanceur, mémoire, ECC/TMR/Reed-Solomon, FDIR, protocole | `systems` | Opus |
| Simulateur, ground station, dashboard, intégration, glue | `builder` | Sonnet |
| Tests en masse, fuzzing, campagne d'injection, doc, CI, scripts | `tester` | Haiku |

Règles de routage :
- Dans le doute, prends le modèle le MOINS cher qui peut réussir ; monte d'un cran après 2 échecs.
- Fable est limité dans le temps : ne l'appelle qu'aux moments marqués ★ dans le déroulé ci-dessous
  ou pour un blocage réel. Jamais pour du code courant.
- Tâches indépendantes (fichiers disjoints) : lance les agents EN PARALLÈLE. Jamais deux agents
  qui écrivent dans les mêmes fichiers.

## Déroulé (3 jours)
**Jour 1 — fondations**
- ★ `strategist` : (a) écrit `docs/SPEC.md` (architecture, interfaces entre crates, modèle de pannes),
  (b) produit un plan de tâches ordonné avec dépendances, (c) fait un PRE-MORTEM : liste ce qui peut
  faire échouer le projet et ce qu'on a oublié (voir checklist ci-dessous). Écrit dans `docs/risks.md`.
- `systems` : noyau firmware (ordonnanceur, modes nominal/éco/survie). `builder` : début du simulateur.
- Fin de jour : le satellite tourne en simulation sans pannes. Sinon, on ne passe pas au jour 2.

**Jour 2 — résilience et outils**
- `systems` : ECC/TMR, watchdog, FDIR, protocole radio. `builder` : injecteur de pannes, ground station, dashboard.
  `tester` : tests unitaires, harnais de fuzzing, CI.
- ★ Fin de jour : `strategist` relit les risques, vérifie que le plan tient, réordonne si besoin.

**Jour 3 — épreuve**
- `tester` : campagne massive d'injection (dizaines de milliers de pannes), métriques brutes.
- ★ `strategist` : analyse des échecs, décide quoi corriger vs quoi documenter, rédige le rapport final.
- `systems` / `builder` : corrigent ce qui est décidé, rien de plus.

## Garde-fous
- **Gate de fin de jour** : ne démarre pas la phase suivante tant que les critères de la phase ne sont pas
  VÉRIFIÉS (commande lancée, résultat lu), pas seulement déclarés par un agent.
- **Gel de périmètre** : à partir de la mi-jour 2, aucune nouvelle fonctionnalité. Tout ajout va dans
  `docs/later.md`.
- **Spec figée** : `docs/SPEC.md` ne se modifie pas sans accord explicite de l'utilisateur. Un changement
  de conception passe par une entrée dans `docs/decisions.md` avec la raison.
- **Honnêteté** : un agent qui échoue ou saute une étape le dit. Un test rouge se rapporte tel quel.
- **Contexte propre** : ne recopie pas de gros fichiers dans les prompts ; donne le chemin et la tâche.
- **Rust** : `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test` verts avant de clore une tâche.
  Le firmware évite `unsafe` sauf justification écrite en commentaire.

## Checklist « ce qu'on oublie souvent » (le strategist la parcourt à chaque phase)
- Les pannes injectées sont-elles reproductibles (graine aléatoire loguée) ?
- Que se passe-t-il quand le watchdog lui-même est touché ? Et le gestionnaire de FDIR ?
- Dépendance cachée : une tâche critique qui attend une tâche non critique (inversion de priorité) ?
- Batterie vide pendant l'éclipse : le satellite repart-il tout seul ?
- Commande reçue corrompue ou rejouée : rejetée proprement ?
- Mesure : comment sait-on que « survit » ? Définition écrite AVANT la campagne, pas après.
- Le taux de survie est-il mesuré sur assez de cas, et les échecs sont-ils catégorisés ?
- Le simulateur est-il biaisé de façon à rendre le firmware meilleur qu'il ne l'est ?
- Reste-t-il du temps réservé au rapport final (au moins 20 % du jour 3) ?

## Format d'une tâche (`tasks/T-NNN-slug.md`, écrit par le manager, complété par l'agent)
```
# T-NNN titre
Agent: <nom>   Modèle: <modèle>   Dépend de: T-xxx
Entrée: fichiers à lire
Sortie attendue: fichiers créés/modifiés
Critère de fin (vérifiable): commande + résultat attendu
## Compte rendu (rempli par l'agent)
Fait / pas fait / écarts / commande lancée et résultat
```
