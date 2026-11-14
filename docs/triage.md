# Triage des échecs de la campagne principale (T-022)

Source : `results/main/` (S = 731051977, 20 000 runs, code du tag `campagne-gelee`), `results/unprotected/`,
`results/double/`, `results/main/failures.md`. Rejeux : `target/release/campaign --seed 731051977 --runs 20000
--replay <graine>` (et `target/unprotected/release/campaign --unprotected …`). Graine V non touchée. Aucun
fichier de `firmware/`, `sim/`, `docs/SPEC.md` modifié par ce triage.

Trois couples (classe, catégorie) non vides, 1868 échecs, tous `E2 THERMAL` en catégorie principale. E0 = 0.

## 1. Décisions par couple

| Couple | n | Cause | Statut de la cause | Décision |
|---|---:|---|---|---|
| F5 / E2 | 40 | faux latch-up : le FDIR isole le réchauffeur sur une lecture de courant hors plage | établie (CSV exhaustif + 3 rejeux + lecture du code) | **CORRIGER** (C1, firmware) |
| F10 / E2 | 400 | OBC coupé par la protection matérielle batterie, réchauffeur non alimenté en éclipse | établie (CSV exhaustif + mesure T-008) | **DOCUMENTER** |
| D1 / E2 (+E3) | 1428 | watchdog mort puis CPU figé : plus rien ne commande les lignes, aucun chemin de redémarrage | établie (SPEC §7 + CSV exhaustif + 3 rejeux) | **DOCUMENTER** |

### 1.1 F5 / E2 (40) : CORRIGER

Faits :
- Les 40 échecs sont exactement les 40 runs `sensor=ILineHeater kind=range` de valeur `30000` ou `32767`
  (40 sur 40 ; les 20 runs à `-30000` sont `OK_NOMINAL`). Aucun autre capteur, aucun autre genre de panne F5
  n'échoue en protégé (1369 autres runs F5 : 0 échec).
- Signature identique sur les 40 : `cnt_latchup = 255` (saturé), `fdir_flags` bit 3 (ligne isolée), `resets = 0`,
  `temp_min_c` entre -15,84 et -15,76 °C (la température d'une éclipse sans réchauffeur), S4 seul critère faux.
- Rejeux protégés 4781019819541052408, 2990178190404449115, 14486980315370451616 : `THERMAL`, même signature.
- Rejeu NON protégé de 4781019819541052408 : `OK_NOMINAL`, `temp_min_c = 1,993`, `cnt_latchup = 0`. Le témoin
  survit 60 fois sur 60 sur ces runs : **ici la protection tue le satellite, le témoin sans FDIR survit**.
- Code : `firmware/src/tasks/fdir.rs`, fonction `latchup`, ligne
  `let over = matches!(hal.sensor(LINE_SENSOR[l]), Ok(i) if i > LU_THRESHOLD_MA[l]);` : aucune borne haute de
  plausibilité. Les capteurs de tension et de température ont une plage (`power::VBATT_RANGE_MV`,
  `TBATT_RANGE_DC`), les capteurs de courant de ligne n'en ont pas. 30 000 mA est pris pour un latch-up ;
  le capteur restant figé, 3 détections en 600 s isolent la ligne, l'isolement est persisté et jamais levé
  (l'opérateur de campagne n'envoie pas `ClearFdir`).

C'est un défaut du firmware, pas une limite : SPEC §4.4 demande « Capteur hors plage / `Err` : immédiat,
capteur invalidé, bit dans `sensor_fault_mask` », et §7 F5 inclut « hors plage ». Le courant physique maximal
du modèle est 250 mA + 800 mA = 1050 mA (annexe A) : 30 000 mA est discriminable sans ambiguïté.

Même défaut, effets non mortels (aujourd'hui `OK_DEGRADED`, pas des échecs) :
- `ILinePayload` hors plage haute : 38 runs, Payload isolée, mode plafonné `Eco` ;
- `ILineRadio` hors plage haute : 38 runs, `cnt_latchup = 255` (Radio coupée 5 s à chaque détection), dont 36
  avec `FDIR_FLAG_CRITICAL` dans la dernière balise (24 `OK_DEGRADED`, 14 `OK_NOMINAL`).
Le correctif C1 les traite aussi ; le gain sur eux se mesure en T-025, il n'est pas promis ici.

Défaut voisin constaté : aucun capteur de courant (`ILine*`, `ISolar`) ne pose jamais de bit dans
`sensor_fault_mask`, ni en `Err` ni hors plage (0 run sur 653). Écart à SPEC §4.4, sans effet sur un verdict
(l'oracle E8 ne lit que les bits tension et température). Traité par C1 partie (b) pour les trois `ILine*`.

### 1.2 F10 / E2 (400) : DOCUMENTER (confirmation de la décision R2 / T-019)

Faits :
- 400 échecs sur 1428, tous avec exactement 1 reset `Brownout`, 400 sur 400 avec S2 vrai (le satellite
  redémarre seul et redevient commandable), S4 seul critère faux. 0 échec parmi les 265 runs sans coupure ;
  400 sur 1163 parmi les runs avec coupure.
- Dépendance à la profondeur de décharge : SoC forcé < 5 % (coupure immédiate) 285 échecs sur 701 ;
  5 à 8 % : 115 sur 422 ; >= 8 % : 0 sur 305.
- Lignes de `runs.csv` identiques octet pour octet en protégé et non protégé (1428 sur 1428) : aucune
  protection du firmware n'intervient, dans un sens ou dans l'autre.
- Mesure T-008 : OBC éteint, la batterie passe 1227 s sous -10 °C sans aucune action du firmware.

Raison : pendant la coupure matérielle le firmware ne tourne pas et le réchauffeur n'est pas alimenté ; S4 est
violé par la physique du modèle. Rien à corriger sans toucher au modèle thermique ou aux seuils, ce qui est
interdit après le gel (SPEC §8.3 règle 5).
Non vérifié, à écrire tel quel au rapport : on n'a pas cherché si un délestage plus précoce du firmware
(avant `SOC_OFF`) éviterait une partie des 115 coupures de la tranche 5 à 8 %. Ce serait une fonctionnalité
nouvelle (gel de périmètre), pas un correctif.

### 1.3 D1 / E2+E3 (1428) : DOCUMENTER

Question posée : est-ce bien hors de portée du design, ou une reprise devrait-elle exister (coupure batterie,
redémarrage à froid) ?

Par la SPEC : §7 ligne D1, « Détection / récupération attendue : **aucune** : perte attendue, à rapporter telle
quelle ». Les seuls chemins de redémarrage du design sont (1) le watchdog matériel, mort par hypothèse dans D1,
(2) `sys_reset` et la panique, qui exigent un CPU qui tourne, (3) la coupure batterie sous `SOC_OFF` suivie du
redémarrage à froid au-dessus de `SOC_ON` (§6), (4) le limiteur OBC de F13, qui ne réagit qu'à une
surintensité. Aucune minuterie de coupure-réarmement indépendante n'est spécifiée, et l'opérateur de campagne
n'envoie aucune commande de récupération (un CPU figé ne l'exécuterait pas).

Par les données (1428 runs de `main` + 1000 de `double`, exhaustif) :
- `resets = 0` sur 2428 runs sur 2428 ; critères `101011` partout (S2 et S4 faux, S3 VRAI).
- Le chemin (3) ne se déclenche jamais : SoC minimal entre 24,3 % et 89,5 % (`main`), 26,6 % et 89,5 %
  (`double`), très au-dessus de `SOC_OFF` = 5 %. Les interrupteurs de ligne restent dans l'état du dernier
  cycle et le panneau recharge : la batterie ne se vide pas, donc pas de redémarrage à froid.
- Mécanisme thermique : réchauffeur figé coupé → froid (1238 runs de `main`, T min -15,8 °C) ; réchauffeur
  figé alimenté → surchauffe (190 runs, T max jusqu'à 74 °C). Jamais les deux.
- Rejeux 16391488761010476146, 1841428591752296710, 8978138193552996872 : conformes (0 reset, plus aucune
  balise après le blocage, `E2+E3`).

Conclusion : le firmware n'y peut rien (il ne s'exécute plus) et le sim est conforme à la SPEC. Le sim ne doit
pas « permettre une reprise » : ce serait modifier le modèle de panne après avoir vu les résultats.
À dire au rapport : (i) la catégorie principale `E2` de D1 vient de l'ordre de priorité de §8.1 ; la cause
réelle est `E3 DEAD`, présente en second sur les 1428 runs ; (ii) D1 est une campagne à part, jamais fondue
dans un taux de panne simple ; (iii) `CpuHang` seul (F9) est récupéré 1428 fois sur 1428 par le watchdog,
dans les deux variantes : c'est la double panne qui tue, pas le blocage.

## 2. Liste fermée de correctifs

Un seul correctif. T-024 (harnais) est VIDE : E0 = 0, 15 rejeux sur 15 identiques, aucune injection sans effet
qui soit un bogue (voir T-019), `oracle.rs` non modifié.

### C1 (firmware, T-023) : plausibilité des capteurs de courant de ligne dans le FDIR

- Gain attendu : 40 échecs F5 sur 40 (F5 protégé 1389 → 1429 sur 1429 si le correctif fait ce qu'il doit) ;
  jusqu'à 62 `OK_DEGRADED` qui redeviennent nominaux. Coût : une constante, une condition, deux tests.
- Fichier : `firmware/src/tasks/fdir.rs` uniquement (fonction `latchup`, constantes, tests du module).
- Changement (a), obligatoire : nouvelle constante ROM `ILINE_RANGE_MA: (i16, i16) = (-100, 2000)`. Dans
  `latchup`, une lecture `Err` ou hors de `ILINE_RANGE_MA` est un capteur INVALIDE pour ce cycle : elle ne
  compte pas comme surintensité (`LU_OVER[l]` remis à 0), ne coupe rien, n'incrémente ni `CNT_LATCHUP` ni
  `LU_RECUR`. Une lecture dans la plage garde exactement le comportement actuel (`i > LU_THRESHOLD_MA[l]`).
  Justification de 2000 mA, à mettre en commentaire : le plus fort courant physique du modèle est
  250 + 800 = 1050 mA (annexe A) ; la borne laisse un facteur ~2.
- Changement (b), séparable : pour ce même cycle, poser le bit `1 << (LINE_SENSOR[l] as u16)` dans
  `ram::SENSOR_FAULT_MASK` (SPEC §4.4 « idem »). `fdir` s'exécute après `power`, qui réécrit le masque à chaque
  cycle : un simple OU suffit, sans état persistant. Si (b) pose la moindre difficulté à l'heure limite, il est
  retiré seul et (a) reste.
- Interdits : toucher à `LU_THRESHOLD_MA`, `LU_SAMPLES`, `LU_RECUR_MAX`, `LU_WINDOW_S`, à la règle
  d'isolement, à `RAM_MAP`, au format de la balise, à la variante `unprotected` (le FDIR y reste neutralisé).
  Pas de levée automatique d'un isolement existant, pas d'invalidation durable du capteur de courant.
- Tests de non-régression exigés (dans `fdir.rs`, variante protégée) :
  1. `c1_iline_out_of_range_is_not_latchup` : pour chacune des 3 lignes et chacune des valeurs `30000`,
     `i16::MAX`, `-30000` et `Err`, 2000 cycles en `Nominal` : `CNT_LATCHUP == 0`, `LINE_ISOLATED == 0`,
     `LU_OFF_LEFT == 0` à chaque cycle, `FDIR_FLAG_CRITICAL` et `FDIR_FLAG_LINE_ISOLATED` absents, ligne
     toujours commandée ; avec (b) : bit du capteur présent dans `SENSOR_FAULT_MASK`.
  2. `c1_real_latchup_still_detected_up_to_range` : surintensité à `ILINE_RANGE_MA.1` (2000 mA) détectée et
     coupée comme avant ; à 2001 mA ignorée ; à nominal + 800 mA détectée pour chaque ligne.
  3. Les tests existants `latchup_cut_within_5_cycles_then_rearmed`, `three_recurrences_isolate_payload_and_cap_eco`,
     `recurrences_outside_window_do_not_isolate`, `radio_is_never_isolated_only_cycled` restent verts SANS
     modification de leur corps.
- Critère vérifiable (avant / après écrit au compte rendu de T-023) :
  - rejeu protégé des 5 graines 4781019819541052408, 2990178190404449115, 14486980315370451616,
    11567456421349545517, 3557172289911602756 : `THERMAL` → survie (`OK_NOMINAL` attendu, sinon le dire) ;
  - rejeu de 9664786027175421902 (`ILinePayload`, aujourd'hui `OK_DEGRADED`) et 15719726885866825989
    (`ILineRadio`, aujourd'hui `OK_DEGRADED`) : verdict rapporté tel quel ;
  - non-régression latch-up réel : rejeu de 11712473911359860057 (F4 Payload), 11486522431347728424 (F4 Radio),
    14102530543474748004 (F4 Heater) : toujours `OK_NOMINAL` avec `cnt_latchup = 1` ;
  - `./scripts/ci.sh` → `CI OK` ; `cargo test -p sim --test gate_day1` vert ;
    `campaign --seed 1 --runs 200 --class none` → 200 `OK_NOMINAL` ; build `unprotected` vert.
  - La preuve complète (F4 1429 sur 1429 conservé, aucune régression sur 20 000 runs) est `results/final/DIFF.md`
    de T-025, pas ces rejeux.

### T-024 (harnais) : aucun correctif

À clore comme tâche vide, avec la mention « oracle.rs : non modifié ».

## 3. Limites à écrire au rapport (T-027), en plus des règles T-019

1. Le chiffre de référence reste `results/main` (avant correctif) : F5 protégé 1389 sur 1429 (97,20 %). Le
   chiffre après C1 est publié À CÔTÉ, jamais à la place, avec la mention « correctif décidé après avoir vu
   les échecs ».
2. C1 est ajusté à des valeurs vues : le modèle ne tire que trois valeurs hors plage (`-30000`, `30000`,
   `32767`). La graine de validation V tire les mêmes trois valeurs ; elle protège contre un ajustement aux
   graines, PAS contre un ajustement à ces valeurs. Un capteur de courant qui mentirait dans la plage physique
   (par exemple 1500 mA) reste indiscernable d'un latch-up : capteur unique par ligne, pas de redondance.
   Limite non mesurée par la campagne.
3. Sur les 60 runs `ILineHeater` hors plage, le firmware protégé faisait PIRE que le témoin (20 survies contre
   60). Un FDIR qui isole une ligne vitale sur la foi d'un seul capteur est un risque de conception ; C1 retire
   le cas mesuré, pas le principe.
4. F5 ne discrimine que par `VBatt*` et `TempBatt*` figés (témoin : 83 échecs sur 214, protégé 0) ; les
   capteurs de courant et `TempObc` ne discriminent pas.
5. F10 : donner 71,99 % (1028 sur 1428) identique dans les deux variantes, S2 vrai sur 400 échecs sur 400,
   la dépendance au SoC forcé (§1.2) et la mesure T-008. D1 : 0 sur 1428 et 0 sur 1000, cause `DEAD`,
   catégorie principale `THERMAL` par ordre de priorité.

## 4. T-030 (balayage exhaustif RAM)

Vaut le coût, sous condition. C'est la seule mesure du projet où F1 peut séparer protégé et non protégé
(campagne : 0 échec F1 des deux côtés, 1 seul échec F2 et 1 seul F8 côté témoin). Coût : un fichier nouveau
(`sim/examples/ram_sweep.rs`), environ 49 000 runs, moins de 10 min de calcul. Conditions : l'écriture peut
partir en parallèle de T-023 (fichiers disjoints, T-024 étant vide le builder est libre) ; l'EXÉCUTION attend
la clôture de T-023 pour tourner sur le code final, un seul build à la fois ; abandon sans regret à 70 % du
jour 3. Attente réaliste à ne pas survendre : quelques échecs sur 24 576 côté témoin (1 sur 8192 au balayage
T-019), 0 attendu côté protégé ; une mesure complémentaire, jamais mélangée aux taux de campagne.

## 5. Hors liste (à verser dans `docs/later.md` par le manager si souhaité)

- Ne jamais isoler le réchauffeur sur récidive de latch-up (comme la Radio), ou lever un isolement après un
  délai : changement de SPEC §4.4.
- Bit `sensor_fault_mask` et plausibilité pour `ISolar`.
- Délestage anticipé avant `SOC_OFF` (F10, tranche 5 à 8 %).
- Minuterie matérielle de coupure-réarmement indépendante du watchdog (D1) : changement de SPEC §6 et §7.
