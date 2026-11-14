# AEGIS — Rapport final de survie sous injection de pannes (T-027)

Ceci est le résultat d'une **SIMULATION**. Firmware, matériel, orbite, énergie, thermique et pannes sont des
modèles logiciels. Aucun matériel réel, aucun environnement radiatif réel, aucune prétention de certification
ou de qualification. Les taux ci-dessous décrivent le comportement d'un programme face à un modèle de panne
écrit par la même équipe ; ils ne prédisent pas la survie d'un satellite.

Tous les chiffres viennent des fichiers cités (`summary.json`, ou dépouillement direct du `runs.csv` voisin
quand c'est dit). Les intervalles sont des intervalles de Wilson à 95 %, lus dans les `summary.json`.

## 1. Résultat

Campagne de validation (`results/validation`, graine V = 990274663 jamais lancée avant, 20 000 runs, une panne
par run, 14 classes de 1428 ou 1429 runs, 0 run exclu) :

> Le firmware protégé survit dans **1429/1429 ou 1428/1428 runs pour 12 des 13 classes de panne simple**
> (Wilson 95 % : [99,73 % ; 100 %] par classe), dans **1012/1428 runs (70,87 % [68,46 ; 73,17])** quand la
> batterie est vidée en éclipse (F10), et dans **0/1428 runs ([0 ; 0,27 %])** pour la double panne D1.
> **Seules trois classes (F3, F4, F5) montrent que les protections du firmware y sont pour quelque chose** ;
> pour les autres, le témoin sans protections survit autant.

Trois réserves à lire avant tout autre chiffre :

1. Le 100 % de F5 est obtenu **après un correctif (C1) décidé en voyant les échecs de la première campagne**.
   Avant correctif : 1389/1429 (97,20 % [96,21 ; 97,94]), `results/main`. Voir §9.
2. Un 100 % sur 1429 runs ne veut pas dire « ne casse jamais » : la borne basse est 99,73 %.
3. Le taux toutes classes confondues (18 156/20 000 en validation) n'a pas de sens physique : la pondération
   entre classes est arbitraire (§8). Il n'est donné qu'au §3.3, avec son détail.

## 2. Définition de « le satellite survit » (SPEC §8, figée avant la campagne)

Un run dure 3 orbites. L'oracle vit dans le simulateur et ne lit que la vérité physique, ce que le sol a reçu
et les TC exécutées ; jamais l'état interne du firmware. Le run survit si et seulement si :

| | Critère | Mesure |
|---|---|---|
| S1 | Pas de perte matérielle vitale | ligne Radio non détruite, batterie non détruite |
| S2 | Vivant et commandable à la fin | dernière passe : >= 1 balise valide reçue et >= 1 `Ping` acquitté |
| S3 | Énergie | OBC alimenté en fin de run et état de charge réel >= 20 % |
| S4 | Thermique | `T_batt` réelle jamais hors [-10, +50] °C plus de 300 s consécutives |
| S5 | Intégrité des commandes | aucune TC exécutée qui ne soit une trame du sol, intacte, exécutée une fois ; aucune TM corrompue acceptée |
| S6 | Stabilité | <= 2 resets pendant la dernière orbite |

Survie = `OK_NOMINAL` + `OK_DEGRADED`. Retour au nominal = `OK_NOMINAL` seul (mode `Nominal`, Payload
alimentée, aucune ligne détruite ni isolée en fin de run). Catégories d'échec E1 à E8 : SPEC §8.1 ; la
catégorie principale est la première applicable dans l'ordre E1…E8. `E8 SILENT_LIE` compte comme échec.

## 3. Résultats par classe

### 3.1 Tableau principal

Colonnes « protégé » : `results/validation/summary.json` (V, après C1), `results/main/summary.json` (S, code du
tag `campagne-gelee`, avant C1), `results/final/summary.json` (S, après C1). Colonne « non protégé » :
`results/unprotected/summary.json` (S, mêmes graines que `main` ; `results/final-unprotected` donne les mêmes
effectifs par classe). **Le témoin non protégé n'a pas été lancé sur V** : la comparaison protégé (V) contre
non protégé (S) se fait entre deux graines différentes ; la comparaison à graines identiques est `main` ou
`final` contre `unprotected`.

| Classe | n | Survie validation (V) | Wilson 95 % | Retour nominal (V) | Survie `main` (S, avant C1) | Survie `final` (S, après C1) | Survie non protégé (S) | Wilson 95 % non protégé | Le témoin chute-t-il ? |
|---|---:|---|---|---|---|---|---|---|---|
| F1 `RamSeu` | 1429 | 1429 (100 %) | [99,73 ; 100] | 1429 | 1429 (100 %) | 1429 (100 %) | 1429 (100 %) | [99,73 ; 100] | **non** |
| F2 `RamMbu` | 1429 | 1429 (100 %) | [99,73 ; 100] | 1429 | 1429 (100 %) | 1429 (100 %) | 1428 (99,93 %) | [99,60 ; 99,99] | **non** (1 run) |
| F3 `RegSeu` | 1429 | 1429 (100 %) | [99,73 ; 100] | 1429 | 1429 (100 %) | 1429 (100 %) | 1069 (74,81 %) | [72,49 ; 76,99] | **oui** |
| F4 `LatchUp` | 1429 | 1429 (100 %) | [99,73 ; 100] | 1429 | 1429 (100 %) | 1429 (100 %) | 15 (1,05 %) | [0,64 ; 1,72] | **oui** |
| F5 `SensorStuck` | 1429 | 1429 (100 %) | [99,73 ; 100] | 1429 | **1389 (97,20 %)** [96,21 ; 97,94] | 1429 (100 %) | 1346 (94,19 %) | [92,86 ; 95,29] | **oui**, faiblement |
| F6 TC corrompue / rejouée / tronquée | 1429 | 1429 (100 %) | [99,73 ; 100] | 1429 | 1429 (100 %) | 1429 (100 %) | 1429 (100 %) | [99,73 ; 100] | **non** |
| F7 `WdgSpurious` / `WdgRegHit` | 1429 | 1429 (100 %) | [99,73 ; 100] | 1429 | 1429 (100 %) | 1429 (100 %) | 1429 (100 %) | [99,73 ; 100] | **non** |
| F8 `FdirHit` / `SchedHit` | 1429 | 1429 (100 %) | [99,73 ; 100] | 1429 | 1429 (100 %) | 1429 (100 %) | 1428 (99,93 %) | [99,60 ; 99,99] | **non** (1 run) |
| F9 `CpuHang` | 1428 | 1428 (100 %) | [99,73 ; 100] | 1428 | 1428 (100 %) | 1428 (100 %) | 1428 (100 %) | [99,73 ; 100] | **non** |
| F10 `BattDeplete` | 1428 | **1012 (70,87 %)** | [68,46 ; 73,17] | 540 (37,82 % [35,34 ; 40,36]) | 1028 (71,99 %) [69,60 ; 74,26] | 1028 (71,99 %) | 1028 (71,99 %) | [69,60 ; 74,26] | **non** (identique) |
| F11 `NvmCorrupt` | 1428 | 1428 (100 %) | [99,73 ; 100] | 1428 | 1428 (100 %) | 1428 (100 %) | 1428 (100 %) | [99,73 ; 100] | **non** |
| F12 `TmCorrupt` | 1428 | 1428 (100 %) | [99,73 ; 100] | 1428 | 1428 (100 %) | 1428 (100 %) | 1428 (100 %) | [99,73 ; 100] | **non** |
| F13 `ObcLatchUp` | 1428 | 1428 (100 %) | [99,73 ; 100] | 1428 | 1428 (100 %) | 1428 (100 %) | 1428 (100 %) | [99,73 ; 100] | **non** |
| D1 `WdgDead + CpuHang` | 1428 | **0 (0 %)** | [0 ; 0,27] | 0 | 0 (0 %) | 0 (0 %) | 0 (0 %) | [0 ; 0,27] | sans objet (0 des deux côtés) |

Retour au nominal dans `main` quand il diffère de la survie : F5 1327/1429 (92,86 % [91,41 ; 94,09]), F10
572/1428 (40,06 % [37,54 ; 42,62]). Dans `final` : F10 572/1428, toutes les autres classes égales à leur survie.
Retour au nominal du témoin non protégé : F1 1428, F2 1425, les autres égaux à leur survie, F10 572.

### 3.2 Ce que chaque ligne permet de dire, et ce qu'elle ne permet pas

**Classes où la protection est mesurée (le témoin chute) :**

- **F4 latch-up** : 1429/1429 contre 15/1429 (témoin : 452 `E1 VITAL_LOSS`, 474 `E2`, 488 `E8`). C'est le
  résultat le plus net du projet : sans la coupure FDIR, la ligne est détruite ou le satellite ment.
- **F3 registres** : 1429/1429 contre 1069/1429 (témoin : 360 `E6 COMMS_LOST`). Le rafraîchissement des
  registres par `health` est mesuré.
- **F5 capteurs** : avant C1, 97,20 % [96,21 ; 97,94] contre 94,19 % [92,86 ; 95,29], à graines identiques :
  les intervalles ne se recouvrent pas, l'écart est de 3 points. Après C1, 1429/1429 contre 1346/1429. La
  discrimination ne vient que des capteurs `VBatt*` et `TempBatt*` figés (témoin : 83 échecs sur 214 runs,
  protégé 0, d'après `docs/triage.md` §3) ; les capteurs de courant et `TempObc` ne discriminent pas. Et sur un
  sous-cas, la protection faisait **pire** que le témoin (§5.1).

**Classes où le taux de survie ne prouve PAS l'efficacité de la protection (le témoin survit aussi) :**

- **F1, F2, F8 (bits en RAM)** : le 100 % ne démontre ni le TMR ni le SECDED. Le témoin sans vote ni ECC
  survit à 1429, 1428 et 1428 runs sur 1429. Cause établie en T-019 : un bit isolé en RAM tue rarement ce
  firmware même sans protection (journal jamais relu en vol, cellules recalculées à chaque cycle), et le témoin
  garde la disposition à trois copies en ne lisant que la copie 0, ce qui dilue les SEU par 3 : c'est une borne
  grossière. Ce qui est disponible à la place d'une preuve par le taux :
  - le nombre de runs où une correction a réellement eu lieu (compteurs `cnt_ecc_corr + cnt_tmr_fix > 0` dans
    la dernière balise, dépouillement de `results/validation/runs.csv`, pannes hors région `Unused`) :
    F1 1251/1282, F2 1271/1301, F8 1380/1429. Dans `results/main/runs.csv` : 1246/1288, 1253/1287, 1395/1429.
    Cela montre que l'injection est appliquée et que le mécanisme s'exécute, pas qu'il sauve le satellite ;
  - les tests unitaires du firmware (`cargo test -p firmware`) ;
  - le balayage exhaustif du §7, mesure à part ;
  - taux F1/F2 hors région `Unused` (validation) : F1 1282/1282, F2 1301/1301 ; avec `Unused` : 1429/1429 chacun.
- **F6 (commandes)** : CRC, longueur et anti-rejeu ne sont pas neutralisés dans le témoin ; identique par
  construction. Aucune commande corrompue ou rejouée n'a été exécutée sur 1429 runs dans chaque campagne
  (0 `E7`), mais la colonne « non protégé » n'apprend rien.
- **F7a `WdgSpurious`, F9 `CpuHang`, F13 `ObcLatchUp`** : la survie est due au watchdog matériel et au
  redémarrage à froid, présents dans les deux variantes. Ces lignes mesurent que le firmware redémarre
  proprement, pas une protection logicielle. **F12** : le rejet des trames descendantes corrompues est fait par
  le sol, hors firmware.
- **F10** : identique dans les deux variantes, ligne pour ligne (§5.2). Aucune protection n'intervient.

**Classes injectées mais NON ÉPROUVÉES (leur 100 % ne dit rien) :**

- **F11 `NvmCorrupt`** : aucun redémarrage ne suit l'injection (0 reset sur les 1428 runs de
  `results/validation/runs.csv`, de même dans `main`), donc la NVM corrompue n'est jamais relue. La campagne
  n'éprouve pas « 3 copies + CRC » ; la seule preuve est dans les tests unitaires de `nvm.rs`.
- **F7b `WdgRegHit`** (714 des 1429 runs F7 en validation, 747 dans `main`) : la corruption de la
  configuration du watchdog est latente ; sans blocage ultérieur rien n'est observable (0 reset watchdog sur
  ces runs). Injecté, non éprouvé.

Rendre ces deux cas observables demande des pannes composées, donc un changement du modèle de panne (SPEC §7),
refusé après le gel et versé dans `docs/later.md`.

### 3.3 Totaux, pour mémoire seulement

Pondération entre classes arbitraire (14 classes équiprobables, dont D1 qui échoue toujours par hypothèse).
Ne pas citer ces chiffres sans le tableau 3.1.

| Campagne | Fichier | Survie | Wilson 95 % | Retour nominal |
|---|---|---|---|---|
| validation (V, protégé, après C1) | `results/validation/summary.json` | 18 156 / 20 000 (90,78 %) | [90,37 ; 91,17] | 17 684 (88,42 %) |
| main (S, protégé, avant C1) | `results/main/summary.json` | 18 132 / 20 000 (90,66 %) | [90,25 ; 91,06] | 17 614 (88,07 %) |
| final (S, protégé, après C1) | `results/final/summary.json` | 18 172 / 20 000 (90,86 %) | [90,45 ; 91,25] | 17 716 (88,58 %) |
| unprotected (S, témoin) | `results/unprotected/summary.json` | 16 313 / 20 000 (81,57 %) | [81,02 ; 82,10] | 15 853 (79,27 %) |

Contrôles (SPEC §8.3) : témoin sans panne 1000/1000 `OK_NOMINAL` (`results/control`, `results/final-control`) ;
E0 `HARNESS` = 0 et 0 panique firmware comptée dans les neuf `summary.json` ; 0 injection jamais appliquée.

## 4. Catégories d'échec et exemples rejouables

Tous les échecs du firmware protégé, dans les trois campagnes, ont `E2 THERMAL` pour catégorie principale.
`E3 DEAD` est présente en second sur tous les runs D1. Aucune occurrence de E1, E4, E5, E6, E7, E8 en protégé.

| Campagne | Échecs | F5 / E2 | F10 / E2 | D1 / E2+E3 |
|---|---:|---:|---:|---:|
| main (S, avant C1) | 1868 | 40 | 400 | 1428 |
| final (S, après C1) | 1828 | 0 | 400 | 1428 |
| validation (V, après C1) | 1844 | 0 | 416 | 1428 |

Exemples (colonne `seed` des `runs.csv` ; rejeu vérifié pour ceux de `main` : 15 rejeux sur 15 identiques
octet pour octet, `results/main/failures.md` §2) :

| Couple | Campagne | Graines de run |
|---|---|---|
| F5 / E2 (avant C1 seulement) | main | 4781019819541052408, 2990178190404449115, 14486980315370451616 |
| F10 / E2 | main, final | 4921580809659328466, 9251429748398028234, 12631572964557384629 |
| F10 / E2 | validation | 3773612032398296621, 18075394285621518929, 17791041549862687635 |
| D1 / E2+E3 | main, final | 16391488761010476146, 1841428591752296710, 8978138193552996872 |
| D1 / E2+E3 | validation | 8541810914824970869, 4947854479547739098, 8585728492560183757 |

Les graines de validation sont lues dans `results/validation/runs.csv` ; leur rejeu individuel n'a pas été
refait pour ce rapport. Commandes au §10.

Échecs du témoin non protégé (`results/unprotected/summary.json`, catégorie principale) : `E1` 452, `E2` 2335,
`E6` 360, `E8` 540.

## 5. Ce qui casse

### 5.1 Corrigé après la première campagne : faux latch-up sur capteur de courant hors plage (F5)

Dans `results/main`, les 40 échecs F5 sont exactement les 40 runs où le capteur de courant du réchauffeur
(`ILineHeater`) renvoie une valeur hors plage haute (30000 ou 32767 mA). Le FDIR la prenait pour un latch-up,
isolait le réchauffeur, et la batterie gelait à l'éclipse suivante (T min ≈ -15,8 °C). Sur les 60 runs
`ILineHeater` hors plage, le firmware protégé survivait 20 fois, **le témoin sans FDIR 60 fois : la protection
tuait le satellite**. C'était un écart à la SPEC §4.4 (capteur hors plage à invalider). Détail :
`docs/triage.md` §1.1. Correctif C1 et ses réserves : §9.

### 5.2 Reste cassé, documenté : batterie vidée en éclipse (F10)

- Validation : 416 échecs sur 1428, survie 70,87 % [68,46 ; 73,17]. `main` et `final` : 400 sur 1428, 71,99 %.
  Identique dans le témoin non protégé (1028/1428, lignes de `runs.csv` identiques octet pour octet, 1428 sur
  1428, `results/main/failures.md` §7).
- Tous les échecs ont exactement 1 reset `Brownout`, S4 pour seul critère faux, et **S2 vrai** (416 sur 416 en
  validation, 400 sur 400 dans `main`) : le satellite redémarre seul et redevient commandable, mais la batterie
  a passé plus de 300 s sous -10 °C pendant la coupure matérielle. Aucun échec parmi les runs sans coupure
  (294 en validation, 265 dans `main`).
- Le retour au nominal en fin de run n'est atteint que dans 540 runs sur 1428 (validation) : 472 survivants
  finissent en mode dégradé.
- Cause : pendant la coupure le firmware ne tourne pas et le réchauffeur n'est pas alimenté. Mesure
  indépendante (T-008, `docs/decisions.md`) : OBC éteint, la batterie passe 1227 s consécutives sous -10 °C sans
  aucune action du firmware. C'est la physique du modèle, pas une faute du firmware ; on a refusé de recalibrer
  le modèle thermique pour sauver la classe.
- Dépendance à la profondeur de décharge (`main`, `docs/triage.md` §1.2) : état de charge forcé sous 5 % : 285
  échecs sur 701 ; 5 à 8 % : 115 sur 422 ; 8 % et plus : 0 sur 305.
- **Non vérifié** : un délestage plus précoce par le firmware éviterait peut-être une partie des coupures de
  la tranche 5 à 8 %. Ce serait une fonctionnalité nouvelle ; elle n'a été ni écrite ni mesurée.

### 5.3 Reste cassé, hors de portée du design : la double panne D1

Voir §6.

### 5.4 Faiblesses connues qui ne sont pas des échecs de campagne

- Un capteur de courant qui mentirait **dans** la plage physique (par exemple 1500 mA) reste indiscernable
  d'un latch-up : un seul capteur par ligne, pas de redondance. Non mesuré.
- Un FDIR qui isole une ligne vitale (réchauffeur) sur la foi d'un seul capteur est un risque de conception ;
  C1 retire le cas mesuré, pas le principe. L'isolement n'est jamais levé sans commande du sol.
- `ISolar` n'a ni plausibilité ni bit dans `sensor_fault_mask` (`docs/triage.md` §1.1), sans effet sur un verdict.

## 6. Doubles pannes (campagne à part)

D1 = watchdog mort, puis processeur figé. SPEC §7 : « aucune récupération attendue : perte attendue, à
rapporter telle quelle ».

| Campagne | Fichier | Survie | Wilson 95 % |
|---|---|---|---|
| double, S, avant C1 | `results/double/summary.json` | 0 / 1000 | [0 ; 0,38 %] |
| double, S, après C1 | `results/final-double/summary.json` | 0 / 1000 | [0 ; 0,38 %] |
| classe D1 dans la campagne mixte | main, final, validation | 0 / 1428 chacune | [0 ; 0,27 %] |

- La catégorie principale affichée est `E2 THERMAL`, uniquement à cause de l'ordre de priorité de SPEC §8.1.
  **La cause réelle est `E3 DEAD`** : le firmware ne s'exécute plus, plus rien ne commande les lignes, la
  batterie finit trop froide ou trop chaude selon l'état figé du réchauffeur. `E3` est présente sur les 1428
  runs (catégories `E2+E3`, critères S2 et S4 faux).
- 0 reset sur tous les runs D1 : aucun des chemins de redémarrage du design ne peut se déclencher (watchdog
  mort par hypothèse ; le panneau continue de charger, état de charge minimal 24,1 % en validation, donc pas de
  coupure batterie ni de redémarrage à froid).
- Ce n'est pas un échec que le firmware pourrait éviter : c'est une limite d'architecture, hors de portée du
  design (SPEC §7 ; pas de minuterie matérielle de coupure-réarmement indépendante du watchdog). Le blocage seul
  (F9) est récupéré 1428 fois sur 1428 ; c'est la double panne qui tue.
- D1 est la seule double panne étudiée. Rien n'est dit sur les autres combinaisons.

## 7. Balayage RAM (mesure complémentaire)

**Mesure décidée après la répétition générale** (T-019/T-030), pour répondre à la question que la campagne ne
tranche pas : un bit isolé en RAM peut-il tuer le satellite, avec et sans protections ? **Ce n'est pas une
campagne et ces chiffres ne se mélangent à aucun taux du §3** : les positions sont imposées (tous les bits),
pas tirées ; la date d'injection est unique (t = 3000 s).

État : T-030 close (commit 6b6f374). Les six fichiers `results/sweep/{protected,unprotected}/sweep_{5501,5502,5503}.csv`
font 8193 lignes chacun, sur le code après C1. Comptes recomptés par le manager. Déterminisme : deux lancements
identiques au `cmp` d'après l'agent (non relancé par le manager). Écart de l'agent : la comparaison à la référence
porte sur le nombre de TC exécutées, pas sur leur liste.

Protocole : pour chacune des 3 graines de monde fixées au gel, un run par bit de l'image RAM (8192 bits),
`RamSeu` à t = 3000 s, même oracle et mêmes seuils que la campagne.

| Variante | Graine | Runs | `OK_NOMINAL` | `OK_DEGRADED` | Échecs | Runs différant de la référence sans panne (hors `Unused`, sur 7424) |
|---|---|---:|---:|---:|---:|---:|
| protégé | 5501 | 8192 | 8192 | 0 | 0 | 7240 |
| protégé | 5502 | 8192 | 8192 | 0 | 0 | 7240 |
| protégé | 5503 | 8192 | 8192 | 0 | 0 | 7240 |
| non protégé | 5501 | 8192 | 8188 | 3 | 1 (`THERMAL`) | 217 |
| non protégé | 5502 | 8192 | 8188 | 3 | 1 (`THERMAL`) | 217 |
| non protégé | 5503 | 8192 | 8188 | 3 | 1 (`THERMAL`) | 225 |

Lecture honnête :

- Protégé : aucun des 8192 bits ne fait perdre le satellite ni ne le dégrade, dans les trois mondes, à cette
  date d'injection. 7240 des 7424 bits hors `Unused` laissent une trace (l'injection est bien appliquée).
- Non protégé : **un seul bit sur 8192** tue le satellite (octet 110, bit 2, région `fdir`), et c'est **le même
  bit dans les trois mondes** ; trois autres bits (octet 61 bit 1 région `mode` ; octet 107 bit 1 et octet 110
  bit 0 région `fdir`) le dégradent, les mêmes aussi.
- Les trois graines ne sont donc **pas trois échantillons indépendants** : le résultat dépend de la position
  du bit, pas du monde. L'effectif utile est 8192 positions, pas 24 576 runs ; aucun intervalle de Wilson n'est
  donné, il serait trompeur.
- Conclusion permise : sur un basculement isolé, les protections RAM font passer de 1 bit mortel et 3 bits
  dégradants à 0 sur 8192, à une date d'injection et avec un témoin qui dilue par 3. Cela confirme l'ordre de
  grandeur de T-019 (environ 1 sur 8192) et explique pourquoi la campagne (≈ 1429 runs par classe) ne peut pas
  discriminer F1/F2/F8. Conclusion non permise : « le TMR/SECDED est validé ». Une seule date, un seul bit par
  run, aucune accumulation d'erreurs (idée reportée dans `docs/later.md`).

## 8. Limites

### 8.1 Limites assumées du modèle (SPEC §7.2, recopié)

- Les basculements de bits touchent l'image RAM, la NVM et les registres simulés. **Ni la pile, ni les
  variables locales d'un cycle, ni le code, ni le compteur ordinal** ne sont injectés ; `CpuHang` et
  la panique en sont des substituts grossiers.
- Les seuils en `const` sont immunisés par construction.
- Physique à un nœud, orbite idéalisée, pas d'attitude, pas de vieillissement.
- Les taux par classe sont conditionnels à « une panne de cette classe a eu lieu » ; la pondération
  entre classes est arbitraire et ne représente aucun environnement radiatif réel.

### 8.2 Limites de la mesure

- **Une panne par run.** Pas d'accumulation de SEU, pas de panne latente suivie d'un second événement. C'est
  pourquoi F11 et `WdgRegHit` ne sont pas éprouvés et pourquoi F1/F2/F8 ne discriminent pas.
- **Dix classes sur treize ne discriminent pas** protégé et non protégé. Le rapport ne peut pas dire que le
  TMR, le SECDED, la NVM à trois copies ou le rafraîchissement du watchdog « marchent » sur la foi de la
  campagne ; il peut dire qu'ils s'exécutent (compteurs) et qu'ils passent leurs tests unitaires.
- **Témoin non protégé grossier** : il garde les trois copies et lit la copie 0 (dilution des SEU par 3) ; il
  conserve CRC, anti-rejeu, watchdog matériel et redémarrage à froid.
- **Témoin non lancé sur la graine de validation** : la colonne « non protégé » vient de la graine S.
- **Durée** : 3 orbites par run. Rien sur la tenue à long terme.
- **Puissance** : ≈ 1429 runs par classe ; un mode d'échec de fréquence inférieure à environ 0,3 % par classe
  passe sous le seuil de détection (borne basse 99,73 %).
- **L'opérateur sol de campagne** n'envoie aucune commande de récupération (pas de `ClearFdir`) : un état
  dégradé le reste.

### 8.3 Biais possibles du simulateur

- **Même équipe, mêmes hypothèses** pour le firmware, le simulateur, le modèle de panne et l'oracle. Un oubli
  commun aux deux côtés est invisible. Aucune validation contre du matériel ou un simulateur tiers.
- **Le modèle de panne est pauvre là où le firmware est testé** : les capteurs « hors plage » ne prennent que
  trois valeurs (-30000, 30000, 32767) ; C1 est ajusté à elles (§9).
- **L'oracle a déjà eu un faux positif** (§9, B2). Il a été corrigé avant la campagne ; rien ne garantit qu'il
  n'ait pas de faux négatif. E8 ne contrôle que tensions, températures et mode annoncé.
- **Physique sans étalonnage** : modèle thermique à un nœud ; il rend F10 sévère (perte par la seule physique)
  et pourrait aussi bien rendre d'autres cas trop faciles.
- **Dates d'injection bornées** : `t_inj` uniforme dans [300 s, 6000 s] ; pas d'injection pendant le démarrage
  ni en fin de run.
- **Biais favorables déjà consignés** (`docs/decisions.md`) : bruit capteur sans queues extrêmes (somme de 12
  uniformes) ; `TempObc` = température batterie (un seul nœud) ; capteurs échantillonnés une fois par tick ;
  sol généreux (10 `Ping` par passe pour 1 requis, resynchronisation automatique du SEQ) ; dans le témoin,
  `LoadSwitches` reste réécrit par power/thermal (écart F3 sous-estimé).

## 9. Ce qui a été corrigé, et quand

Dit explicitement, dans l'ordre :

1. **Après la répétition générale, avant tout run de campagne (gate J2, NO-GO levé par deux correctifs) :**
   - **B2 (T-029), correction de l'oracle.** Faux positif `E7` : une trame corrompue rejetée par le firmware,
     dont le numéro de séquence basculé valait un numéro futur, se voyait attribuer plus tard l'exécution de la
     vraie trame. Deux runs F6 de la répétition concernés (run_id 1195 et 1279, `CMD_INTEGRITY` → `OK_NOMINAL`
     dans les deux variantes) ; aucune autre ligne de la répétition n'a changé. La définition de S5 n'a pas changé.
   - **B1 (T-028)** : ajout des compteurs de la dernière balise dans `runs.csv` (exigés par SPEC §8.2).

   Puis gel : seuils, oracle, modèle de panne, graines S et V écrits dans `docs/decisions.md` avant le premier run.
2. **Première campagne** (`results/main`, code du tag `campagne-gelee`) : c'est la **référence non retouchée**.
   F5 protégé : 1389/1429 (97,20 %).
3. **Après avoir vu ses échecs : C1 (T-023, commit a59b7f5), un seul correctif, dans le firmware.** Une lecture
   de courant de ligne en erreur ou hors de [-100, 2000] mA n'est plus comptée comme surintensité par le FDIR.
   Ni l'oracle, ni un seuil, ni le modèle de panne n'ont été touchés.
4. **Après C1** : `results/final` (même graine S) et `results/validation` (graine V). F5 : 1429/1429 dans les
   deux. `results/final/DIFF.md` : 0 graine survivante dans `main` qui échoue dans `final` ; 40 dans l'autre
   sens. Les 62 `OK_DEGRADED` de F5 dans `main` sont `OK_NOMINAL` dans `final` (dépouillement des deux `runs.csv`).

**Réserve sur C1, à ne pas perdre :** le correctif a été écrit en connaissant les cas qui échouaient. La graine
de validation protège contre un ajustement aux graines de S, **pas contre un ajustement au modèle de panne** :
V tire les mêmes trois valeurs hors plage, que C1 filtre par construction. Le 100 % de F5 en validation mesure
donc « C1 traite les trois valeurs que le simulateur sait produire », pas « le firmware résiste aux capteurs de
courant défaillants ». Le chiffre obtenu sans connaissance préalable des échecs est celui de `main` : 97,20 %.

Pour les 13 autres classes, le code qui a produit `validation` n'a subi aucun correctif motivé par leurs
résultats ; leurs chiffres en V sont une confirmation sur graine non vue.

## 10. Reproduire

Déterminisme : graine du run i = `splitmix64(graine_campagne XOR i)` ; deux lancements donnent des `runs.csv`
identiques octet pour octet (contrôlé sur les 500 premiers runs : `results/main-check500`, `results/final-check500`).

```sh
# Vérifications du dépôt
./scripts/ci.sh                         # fmt, clippy, tests ; attend "CI OK"

# Campagne après C1 (témoin sans panne, mixte protégé, mixte non protégé, double, contrôle 500), S = 731051977
AEGIS_PREFIX=final2 ./scripts/campaign.sh        # écrit results/final2*, refuse d'écraser un dossier existant

# Validation, V = 990274663
cargo build --release -p sim --bin campaign
target/release/campaign --seed 990274663 --runs 20000 --out results/validation2

# Référence avant C1 : se placer sur le code du tag, puis mêmes commandes
git checkout campagne-gelee
cargo build --release -p sim --bin campaign
cargo build --release --target-dir target/unprotected -p sim --features unprotected --bin campaign
target/release/campaign --seed 731051977 --runs 20000 --out results/main2
target/unprotected/release/campaign --unprotected --seed 731051977 --runs 20000 --out results/unprotected2
target/release/campaign --seed 731051977 --runs 1000 --class none --out results/control2
target/release/campaign --seed 731051977 --runs 1000 --class double --out results/double2

# Rejouer un run (graine de run lue dans runs.csv)
target/release/campaign --seed 731051977 --runs 20000 --replay 4781019819541052408
target/unprotected/release/campaign --unprotected --seed 731051977 --runs 20000 --replay 4781019819541052408

# Tableaux d'échecs
./scripts/failures.sh results/main

# Balayage RAM (§7)
cargo run --release -p sim --example ram_sweep -- --out results/sweep2 --seeds 5501,5502,5503
cargo run --release --target-dir target/unprotected -p sim --features unprotected --example ram_sweep -- --out results/sweep2 --seeds 5501,5502,5503
```

Les commandes « référence avant C1 » et « balayage » sont reconstituées à partir de `scripts/campaign.sh`, de
`scripts/rehearsal.sh` et de l'en-tête de `sim/examples/ram_sweep.rs` (options vérifiées dans ces fichiers) ;
elles n'ont pas été relancées pour ce rapport. Le rejeu de la graine 4781019819541052408 donne `THERMAL` sur le
code du tag et `OK_NOMINAL` après C1 (`docs/decisions.md`, entrée T-023).

Provenance : `results/main/VERSION` = 7c4e865 (firmware et sim identiques au tag `campagne-gelee`) ;
`results/final/VERSION` = e8bb46f (firmware et sim identiques à a59b7f5). Les `summary.json` portent un suffixe
`-dirty` dû à des fichiers non suivis hors `firmware/` et `sim/src` (`docs/decisions.md`, entrées T-020 et T-025).

Rédaction : texte rédigé par le strategist (T-027) ; l'outil d'écriture du sous-agent ayant refusé de créer ce
fichier, il a été posé par le manager sans changement de fond, §7 mis à jour après la clôture de T-030.

## 11. En une page

- Mesuré, à l'intérieur de ce modèle : la coupure FDIR sur latch-up (F4, 100 % contre 1 %), le
  rafraîchissement des registres (F3, 100 % contre 75 %), la redondance et la détection de figeage sur les
  capteurs de tension et de température batterie (F5).
- Observé sans être attribuable aux protections : survie aux bits isolés en RAM, aux commandes corrompues,
  aux resets intempestifs, au blocage CPU, au latch-up OBC, aux trames descendantes corrompues.
- Non éprouvé : NVM à trois copies, corruption de la configuration du watchdog.
- Cassé et le restera sans changer le design : batterie vidée en éclipse (416 pertes thermiques sur 1428 en
  validation, le satellite redémarre seul dans tous ces cas) ; watchdog mort puis blocage (0 survie sur 1428).
- Corrigé en cours de route : un faux positif de l'oracle (avant la campagne), un faux latch-up du FDIR (après
  la première campagne, avec la réserve du §9).
- Sources : `results/{main,final,validation,unprotected,final-unprotected,control,final-control,double,final-double}/summary.json`,
  les `runs.csv` voisins, `results/final/DIFF.md`, `results/main/failures.md`, `results/sweep/`, `docs/triage.md`,
  `docs/decisions.md`, `docs/SPEC.md` §7 et §8.
