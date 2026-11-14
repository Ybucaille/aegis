# AEGIS — PRE-MORTEM (phase 0, 2026-11-14)

Hypothèse de travail : on est à J+3, il n'y a pas de rapport chiffré défendable. Pourquoi ?
Rien ci-dessous n'est mesuré : aucun code n'existe encore. Les calculs sont des ordres de grandeur faits
à la main sur l'annexe A de la SPEC ; ils servent à savoir quoi mesurer tôt, pas à conclure.

Échelles : probabilité et impact en F (faible) / M (moyen) / É (élevé). « Tâche » = où la parade est portée.
Mise à jour prévue en T-019 (colonne État à remplir alors).

## 1. Les trois scénarios d'échec les plus plausibles

1. **Le témoin sans panne n'atteint jamais 100 % `OK_NOMINAL`**, on le découvre à la répétition (fin J2), on
   retouche énergie, thermique et oracle dans l'urgence, la campagne part en retard sur une physique mal comprise.
2. **Le firmware du jour 2 n'est pas fini** (trois tâches L à la suite sur Opus, ralenties par la règle « tout
   dans `ram` »), la campagne mesure un firmware sans FDIR, et le rapport compare « protégé » à « non protégé »
   sans écart visible.
3. **Les chiffres sortent mais ne veulent rien dire** : classes à 100 % parce que la panne ne mord pas (dilution
   dans les tampons, pannes synchrones au tick), ou classe à ~0 % par construction de la physique (F10), et le
   rapport ne sait pas séparer « firmware » de « modèle ».

## 2. Registre des risques

| # | Risque | P | I | Signal précoce observable | Parade / décision | Tâche |
|---|---|---|---|---|---|---|
| R1 | **Calibration énergie : témoin non nominal.** Bilan à la main : nominal = 1,8 W continu + balise 0,3 W moyen = 3,3 Wh/orbite **hors réchauffeur** ; un réchauffeur à 100 % en éclipse ajoute 0,875 Wh → 4,2 Wh > 4,0 Wh produits. Or la SPEC impose un refroidissement rapide (< -10 °C en < 1500 s), donc un rapport cyclique élevé. Le run finit à la même phase qu'il a commencé : s'il finit en fin d'éclipse sous 50 % de SoC, il est en `Eco` → `OK_DEGRADED` → témoin < 100 %. | É | É | T-003 : rapport cyclique du réchauffeur en éclipse (thermostat idéal) > 60 % ; T-008 : SoC en fin d'éclipse < 55 % sur la graine 1 ; T-009 : un coin de la grille (SoC 60 % x 8 phases) non nominal | Mesurer **en J1** et non en J2 : grille de coins déterministe en T-009, bilan ouvert en T-003. Si rouge : recalibrer l'annexe A (puissance du réchauffeur, constante thermique, SoC initial mini) par entrée `decisions.md` **avant T-010**. Ordre de préférence : baisser la puissance du réchauffeur, puis relever le SoC initial mini, en dernier la puissance solaire. | T-003, T-008, T-009 |
| R2 | **F10 `BattDeplete` perdu par construction (S4), pas par le firmware.** OBC coupé sous 5 % → plus de réchauffeur → d'après la contrainte de calibration, `T_batt` < -10 °C en < 1500 s. Une déplétion avec plus de ~1800 s d'éclipse restante viole S4 (> 300 s) quoi que fasse le firmware. « Repart tout seul » sera vrai, « survit » sera faux. | É | M | T-008 : test de démarrage à froid, durée passée sous -10 °C OBC éteint | **Décision à prendre par le manager avant le gel (T-019)**, sur la mesure de T-008 : (a) accepter et l'écrire tel quel dans le rapport (taux F10 = borne de la physique, donner aussi le taux « redémarre et redevient commandable ») ; ou (b) recalibrer la thermique. Je recommande (a) : ne pas tordre la physique pour sauver une classe. Interdit après avoir vu la campagne. | T-008, T-019, T-027 |
| R3 | **Ambiguïtés de l'oracle découvertes après les résultats.** (i) « dernière passe » tronquée par `T_RUN` (phase aléatoire) : 20 s de passe, balise à 60 s en `Survie` → S2 faux sans faute du firmware, y compris dans le témoin ; (ii) S6 : quels resets comptent (coupure matérielle, `Commanded`) ; (iii) S5 côté TM : « corrompue acceptée » ne se mesure que par comparaison d'octets. | É | É | T-013 : le test « vol sans panne 1..=20 » passe mais la grille de phases de T-009 donne un `COMMS_LOST` | Interprétations fixées **maintenant**, à consigner par le manager dans `decisions.md` avant T-013 : dernière passe = dernière passe **entièrement contenue** dans `[0, T_RUN]` ; S6 compte tout redémarrage de l'OBC quelle qu'en soit la cause ; S5 = comparaison octet à octet avec le registre des trames émises, sans repasser par `proto`. | T-013 |
| R4 | **Goulot Opus, chaîne firmware J2** (T-010 → T-011 → T-012, 6 à 12 h d'agent en série, plus coupures de quota). Si T-012 glisse, pas de FDIR à la répétition, gate J2 ratée. | É | É | T-010 non close à 35 % du J2 ; T-011 non close à 65 % | (1) T-010 réserve dans `ram.rs` toutes les cellules de T-011 et T-012 → les deux deviennent lançables **en parallèle** (fichiers disjoints, liste dans les tâches) si le signal se déclenche. (2) Repli de modèle : T-011 (NVM, TC) peut passer à Sonnet, T-012 reste Opus. (3) Ordre de sacrifice écrit : `DumpEvents` + journal, puis `PowerCycle`, puis détection de capteur figé (garder hors plage/`Err`). Jamais : latch-up, tempête de resets, battement, NVM. Tout ce qui est coupé va dans le rapport. | T-010, T-011, T-012 |
| R5 | **Contrainte « tout l'état dans `ram: [u8; 1024]` »** : code verbeux, lent à écrire ; tentation de contourner (un `static AtomicU32` ne demande pas d'`unsafe`), ce qui fausserait toute la campagne sans que rien ne casse. Tension inverse : pour tenir `Unused` < 10 %, remplissage par journal et tampons. | M | É | T-005 dépasse 4 h ; `grep` de `static`/`Atomic`/`Cell` non vide dans `firmware/src` | Contrôle par `grep` ajouté au critère de T-005 (à reprendre dans `ci.sh` par le manager). Une seule abstraction de cellule, écrite d'abord, testée seule. Si T-005 déborde : `payload` et `thermal` réduits au minimum, pas de raccourci sur `ram.rs`. | T-005 |
| R6 | **Dilution : F1/F2 survivent parce que la plupart des bits sont inoffensifs**, pas parce que la protection marche. L'état critique tient en quelques dizaines d'octets ; le reste du kilo-octet est journal et tampons. La règle 3 de §8.3 n'isole que `Unused`. Conséquence : écart protégé / non protégé faible, taux F1 flatteur. | É | M | Répétition : survie F1 non protégée > 90 % | `summary.json` ventile F1/F2 par **type de région** (`Sched`, `Mode`, `Fdir`, `Proto`, `Tm`, `Log`, `Unused`), protégé et non protégé. Le rapport cite le taux par région, pas seulement le taux uniforme. Si l'écart reste faible : le dire (§8.3 règle 2 l'exige déjà). | T-015, T-018, T-027 |
| R7 | **Protocole à implémentation unique** : une erreur d'encodage (boutisme, bornes du CRC, décalage) est symétrique sol/bord, donc invisible à tous les tests d'aller-retour et à la campagne. Impact limité sur le taux de survie, réel sur la crédibilité de « conforme à la SPEC §5 ». | M | M | Un vecteur doré échoue en T-002 ; écart au test différentiel de T-017 | Vecteurs dorés calculés **hors du projet** (`zlib.crc32`, voir annexe) exigés en T-002 ; en T-017, encodeur de référence indépendant écrit depuis la SPEC seule, test différentiel. Reste accepté et à écrire : l'interprétation de la charge utile (opcodes, champs de la balise) n'a qu'un seul lecteur. | T-002, T-017 |
| R8 | **Coût et fragilité du témoin `unprotected`** : `cfg` dispersés = deux firmwares à maintenir ; recompilation complète à chaque bascule de feature ; unification des features par cargo (`--workspace` avec la feature contamine le binaire protégé) ; et sens du témoin : FDIR neutralisé → latch-up Radio = perte triviale, alors que panique + reset « soignent » beaucoup de SEU RAM même sans protection. | M | M | `cargo test -p firmware --features unprotected` rouge après T-011 ou T-012 ; durée de `rehearsal.sh` dominée par la compilation | `cfg(feature = "unprotected")` confiné à des points uniques (`tmr.rs`, `ecc.rs`, entrée de `fdir`, rafraîchissement de `health`), jamais dans les tâches. Répertoire cible séparé pour le binaire non protégé. `summary.json` écrit quelle variante a tourné. Accepté : le témoin est une borne grossière, à présenter comme telle. | T-010, T-012, T-015 |
| R9 | **État persistant empoisonné → boucle de boot permanente.** La RAM est effacée au reset (donc le reset guérit presque tout), pas la NVM. Une valeur corrompue en RAM puis écrite en NVM avec un CRC **valide** survit à tous les resets. Exemple concret : `last_tc_seq` proche de `u32::MAX` → `last + 1024` déborde → panique (`overflow-checks`) à chaque TC → `BOOT_LOOP` définitif, et plus aucune TC acceptable. | M | É | Test T-011 « NVM à CRC valide et contenu arbitraire » panique | Arithmétique saturante sur la fenêtre anti-rejeu ; bornage de chaque champ relu de la NVM ; test : 1000 blocs à CRC valide et contenu aléatoire → `boot` + 300 cycles sans panique. Le cas `last_tc_seq` saturé (plus de TC possible sans `NVM_LOST`) est une limite à documenter, pas à corriger. | T-011 |
| R10 | **Pas d'instantané du code** : aucune tâche ne commite. Le dépôt git n'existe pas encore (créé en T-001). La règle « correctif non vert retiré à 70 % du J3 » et la preuve « code gelé pendant la campagne » supposent un retour arrière possible. | M | É | `git log` vide à la gate J1 | Commit **local** (jamais de push) par le manager à chaque gate et avant T-023/T-024 ; à défaut, copie datée de `firmware/` et `sim/`. À décider et consigner par le manager. | README, T-020, T-023 |
| R11 | **Débit de campagne insuffisant.** Total prévu ≈ 104 000 runs x 17 100 ticks ≈ 1,8 milliard de cycles (main, non protégé, final, validation, témoins). À 20 runs/s par fil et 16 fils (machine actuelle) : une dizaine de minutes. À 2 runs/s : hors budget. Aggravant : des milliers de messages de panique sur stderr. | F | É | Débit mesuré en T-015 < 20 runs/s par fil | Hook de panique silencieux dans `campaign` ; si débit trop bas : profiler avant de réduire, puis réduire dans cet ordre : `final-unprotected`, `final-double`, jamais `validation` ni les effectifs par classe. | T-015, T-019 |
| R12 | **Biais du simulateur : pannes synchrones au tick.** Une panne est appliquée à l'étape 1, `health` réécrit les registres à l'étape 3, le watchdog n'est évalué qu'à l'étape 4 : un SEU de registre (F3, `WdgRegHit`) ne peut jamais produire d'effet de course. Même chose pour un SEU RAM : jamais « au milieu » d'un cycle. | É | M | F3 et `WdgRegHit` à 100 % en protégé dès la répétition | **Accepté**, à documenter dans le rapport avec §7.2 (limite du modèle, pas un mérite du firmware). T-019 vérifie que la variante non protégée, elle, chute sur ces classes ; sinon l'injection est sans effet. | T-019, T-027 |
| R13 | **Tâche non critique morte en silence** : un compteur non protégé de `telemetry` ou `comms_rx` corrompu (période de balise énorme) coupe la radio alors que les 4 tâches critiques tournent et que le watchdog est rechargé → `COMMS_LOST` sans aucune détection. La SPEC ne met pas ces compteurs en TMR. | M | M | Répétition : `COMMS_LOST` sur F1 en région `Tm`/`Proto` | Probablement un **vrai résultat** de la campagne. Ne pas l'anticiper par une fonctionnalité (gel de périmètre) ; parade peu coûteuse admise en T-005 : compteurs de période bornés à la lecture (`min(valeur, période)`). Sinon candidat de triage T-022. | T-005, T-022 |
| R14 | **Ambiguïtés capteurs → `SILENT_LIE` par construction.** (i) Le « drapeau » de désaccord A/B n'est pas dit être dans `sensor_fault_mask` ; s'il n'y est pas, tout F5 sur une tension donne `SILENT_LIE`. (ii) « Le redondant a varié de >= 30 mV » : avec un bruit de 5 mV, l'étendue sur 120 s atteint ~30 mV par le bruit seul, mais un écart début-fin ne l'atteint pas (16 mV pour 2 W) → détection ou non selon la lecture. (iii) Politique de désaccord sur les températures non écrite. | M | M | T-012 : test « capteur figé » ne déclenche pas à 120 s ; répétition : F5 en `SILENT_LIE` | Fixé dans T-012 : désaccord → bit dans `sensor_fault_mask` ; « varié » = étendue max - min sur la fenêtre ; température en désaccord → la plus **froide** pour le thermostat + bit. Choix consignés dans `decisions.md` par l'agent. | T-012 |
| R15 | **Agent Haiku qui « arrange » une mesure** (seuil retouché, run relancé, test affaibli) ou script de campagne faux, sur les tâches de mesure T-009, T-018, T-020, T-025. | M | É | Diff non vide dans `firmware/` ou `sim/src` après une tâche `tester` ; assertions moins strictes que la tâche | Déjà couvert par « le manager lance lui-même la commande ». Ajouter : le manager regarde la liste des fichiers modifiés après chaque tâche `tester`. Monter à Sonnet après un écart, pas deux. | toutes les tâches `tester` |
| R16 | **Correctifs du J3 ajustés aux graines vues**, régressions non vues, rapport sur un code différent de celui mesuré. | M | É | `DIFF.md` : graines passées de survie à échec | Couvert : graine de validation jamais vue (T-025), liste fermée de 5 correctifs, heure limite à 70 %, chiffre de tête = validation. Rien à ajouter. | T-022 à T-027 |
| R17 | **Indisponibilité de Fable ou d'Opus** en cours de route (quota). | M | M | Erreur de modèle à l'appel | Règle de `CLAUDE.md` (Fable → Opus → Sonnet, consigné). Les trois tâches ★ restantes (T-019, T-022, T-027) ont des entrées et critères assez écrits pour être reprises par Opus. | T-019, T-022, T-027 |
| R18 | **Dashboard qui mange le jour 2.** | M | F | T-016 non close quand T-015 l'est | Couvert : première tâche sacrifiée, repli `GET /` + `/api/state`. | T-016 |
| R19 | **`StepReport.executed_tc` est déclaré par le firmware lui-même** : un firmware corrompu peut exécuter sans le dire. S5 est donc une borne optimiste. | F | F | — | Accepté ; une ligne dans les limites du rapport. | T-027 |

## 3. Checklist de `CLAUDE.md`, point par point

| Point | Couvert par | Trou restant |
|---|---|---|
| Pannes reproductibles, graine loguée | SPEC §7.1 ; T-003 (PRNG figé) ; T-015 (`--replay`, CSV identique 1 fil / N fils) ; T-018 (`determinism.rs`) ; T-020 (relance de 500 runs) | Version du code : dépend de R10 (pas de commit). Reproductibilité garantie sur la même machine et le même compilateur seulement (flottants) : à écrire dans le rapport. |
| Watchdog touché ; FDIR touché | F7, F8, F9, D1 ; T-012 (battement gelé → plus de rechargement ; test de masse sur `Fdir` et `Sched`) | R12 : `WdgRegHit` ne peut pas mordre en protégé (ordre du tick). R13 : la surveillance ne couvre que les tâches critiques. D1 est une perte assumée. |
| Inversion de priorité | SPEC §4.1 (ordre fixe, invariant critique / non critique, rechargement conditionnel) ; T-005 | Une **panique** dans une tâche non critique emporte tout le cycle : les critiques sont otages via le reset. Guéri par l'effacement de la RAM, sauf cause persistante en NVM (R9). Pas de modèle de temps d'exécution : un dépassement de budget n'existe que sous la forme `CpuHang` (limite §7.2). |
| Batterie vide en éclipse, redémarrage autonome | F10 ; coupure / reprise `SOC_OFF` / `SOC_ON` (T-003, T-006) | **Non testé de bout en bout avant le J2** dans le plan initial → ajouté en T-008. Et R2 : le redémarrage aura lieu mais S4 sera probablement violé. |
| Commande corrompue ou rejouée | SPEC §5 ; T-005, T-011 (rejeu après reset), T-017 (fuzzing), F6, S5 | R7 (bogue symétrique). Après `NVM_LOST`, `last_tc_seq` revient à 0 : d'anciennes trames redeviennent rejouables — double panne, hors campagne principale, à écrire. Pas d'authentification (déjà dit en §5). |
| « Survit » défini avant la campagne | SPEC §8 figée ; T-013 écrit avant T-014 ; gel en T-019 | R3 : trois ambiguïtés à lever avant T-013, pas après. |
| Assez de cas, échecs catégorisés | §8.3 règle 4 (>= 20 000, >= 1000 par classe, Wilson) ; E0–E8 ; T-021 | R6 (dilution) : ventilation par région ajoutée. Avec 1000 runs par classe, un taux observé de 100 % ne garantit que ~99,6 % (borne basse de Wilson) : aucune phrase « ne casse jamais ». |
| Simulateur biaisé en faveur du firmware | Oracle sur la vérité physique seule (T-013, `grep` sur `ram_mut`) ; témoin non protégé ; test d'effet physique par classe (T-014) ; agents différents pour `firmware` et `sim` | R12 (synchronisme), R2 (biais **contre** le firmware), R19. Marges généreuses à dire : latch-up détecté en ~3 s pour `T_SEL` = 30 s ; canal sans perte ; le sol se resynchronise seul sur `last_tc_seq`. Le reset efface toute la RAM : c'est le premier mécanisme de survie, avant TMR et ECC — le rapport doit donner le nombre de resets par classe. |
| Temps réservé au rapport | Budget J3 : 25 % ; T-027 démarre à 75 % quoi qu'il arrive | Rien. Seul risque : R17. |

Compléments à la checklist (oubliés par elle) :
- La physique peut-elle, seule, faire échouer un critère de survie (R1, R2) ? À mesurer en J1.
- Un état persistant peut-il empêcher la guérison par reset (R9) ?
- Chaque classe à 100 % est-elle réellement injectée (T-019) et chaque taux uniforme cache-t-il une dilution (R6) ?
- Peut-on revenir à l'état du code mesuré (R10) ?
- Le témoin non protégé est-il compilé à part et identifié dans ses résultats (R8) ?

## 4. Décisions demandées au manager (à consigner dans `docs/decisions.md`)

1. Interprétations de l'oracle (R3) : dernière passe complète ; S6 = tout redémarrage ; S5 octet à octet. Avant T-013.
2. Instantanés du code (R10) : commit local à chaque gate, ou copie datée.
3. F10 (R2) : accepter la perte thermique ou recalibrer, **sur la mesure de T-008**, au plus tard en T-019.
4. Recalibrage éventuel de l'annexe A (R1) : après T-009, avant T-010.
5. Déclencheur de la mise en parallèle T-011 / T-012 (R4) : T-010 non close à 35 % du J2.

Aucune de ces décisions ne demande de modifier `docs/SPEC.md`. Si le manager juge que (1) est un changement de
définition plutôt qu'une précision, c'est un cas d'arrêt n° 3 de `CLAUDE.md` (accord de l'utilisateur).

## 5. Tâches modifiées par ce pre-mortem

Chaque fichier porte une ligne « Ajout PRE-MORTEM (Rn) ». T-003 (surcharges SoC / phase, mesure du rapport
cyclique), T-002 (vecteurs dorés), T-005 (`grep` d'état caché, compteurs bornés), T-008 (démarrage à froid,
mesures de calibration), T-009 (grille de coins), T-010 (vote bit à bit, copies espacées, cellules réservées,
`cfg` confiné), T-011 (NVM empoisonnée, saturation), T-012 (ambiguïtés capteurs, option parallèle, ordre de
sacrifice), T-013 (interprétations S2 / S5 / S6), T-015 (ventilation par région, cible séparée, hook de panique),
T-017 (encodeur de référence indépendant), T-018 (contrôles supplémentaires), `tasks/README.md` (section d'ajouts).

## Annexe — vecteurs dorés de trame (calculés avec `zlib.crc32`, hors du projet)

Trame = SYNC, TYPE, SEQ gros-boutiste, LEN, charge utile, CRC-32 sur `[2, 8+LEN)`. La charge utile est opaque ici.

| Cas | Octets (hex) |
|---|---|
| TYPE `0x11`, SEQ 1, charge utile `01` | `ae 61 11 00 00 00 01 01 01 33 1b 23 e4` |
| TYPE `0x12`, SEQ 7, charge utile `00 00 00 01 01 00` | `ae 61 12 00 00 00 07 06 00 00 00 01 01 00 ea 3a d2 59` |
| TYPE `0x11`, SEQ `0x01020304`, LEN 0 | `ae 61 11 01 02 03 04 00 88 0b ea fb` |
