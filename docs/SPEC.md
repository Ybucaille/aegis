# AEGIS — SPEC (contrat entre agents)

Statut : rédigée en phase 0 (2026-11-14). **Figée** dès validation par le manager : toute modification
passe par l'utilisateur. Seule l'annexe A (valeurs numériques) est calibrable par entrée dans
`docs/decisions.md`, et uniquement jusqu'au lancement de la campagne.

C'est une simulation sur PC. Aucune prétention de certification, aucun matériel réel.

---

## 1. Vue d'ensemble

```
            +---------------------- sim (std) -----------------------+
            |  World : orbite, énergie, thermique, canal radio,      |
            |  matériel simulé (registres, NVM, watchdog, capteurs), |
            |  injecteur de pannes, oracle de survie                 |
            |        |  impl firmware::Hal                           |
            |        v                                               |
            |  firmware (no_std, lib)  <--octets-->  ground (lib)    |
            +--------------------------------------------------------+
                         ^ lib                       ^ lib
                    bin `campaign`              dashboard (bin HTTP)
```

- Un seul processus, un seul fil d'exécution par run. Pas de réseau entre sim, firmware et ground.
- Temps discret : **1 tick = 1 s simulée**. À chaque tick le sim met à jour la physique, puis appelle
  `Firmware::step` une fois, puis fait avancer le canal radio et la ground station.
- Tout est déterministe à partir d'une graine `u64`.

## 2. Workspace cargo

| Crate | Type | Dépend de | Contenu |
|---|---|---|---|
| `firmware` | lib `#![no_std]`, `#![forbid(unsafe_code)]` | rien | ordonnanceur, modes, FDIR, TMR/ECC, protocole (`proto`), trait `Hal` |
| `sim` | lib + bin `campaign` | `firmware`, `ground` | `World`, matériel simulé, pannes, oracle, campagne |
| `ground` | lib | `firmware` (module `proto` seulement) | décodage TM, opérateur automatique, file de TC |
| `dashboard` | bin | `sim`, `ground`, `firmware` | serveur HTTP `std::net` + page statique embarquée |

Règles d'outillage :
- Rust 1.99 du système, compilation native uniquement. `no_std` est vérifié par l'attribut de crate
  (les tests du firmware peuvent utiliser `std` via `#[cfg(test)]`).
- **Zéro dépendance externe par défaut** (PRNG, CRC, JSON et HTTP écrits à la main, ils sont petits).
  Toute dépendance ajoutée = entrée dans `docs/decisions.md` et doit se compiler hors ligne.
- Pas d'allocation dans `firmware` (pas de `alloc`). Tampons de taille fixe.
- Profil de campagne : `--release` avec `overflow-checks = true` et `panic = "unwind"` (voir 4.4).

## 3. Interface firmware <-> matériel : `firmware::hal`

```rust
pub trait Hal {
    fn sensor(&mut self, id: SensorId) -> Result<i16, HalError>;
    fn reg_read(&mut self, r: Reg) -> u16;
    fn reg_write(&mut self, r: Reg, v: u16);
    fn wdg_kick(&mut self, key: u8);
    fn radio_recv(&mut self, buf: &mut [u8; MAX_FRAME]) -> Option<usize>; // une trame par appel
    fn radio_send(&mut self, frame: &[u8]) -> Result<(), HalError>;
    fn nvm_read(&mut self, off: u16, buf: &mut [u8]) -> Result<(), HalError>;
    fn nvm_write(&mut self, off: u16, data: &[u8]) -> Result<(), HalError>;
    fn reset_cause(&mut self) -> ResetCause;
    fn sys_reset(&mut self); // prend effet à la fin du step courant
}

pub enum SensorId { VBattA, VBattB, ISolar, ILinePayload, ILineRadio, ILineHeater,
                    TempBattA, TempBattB, TempObc }      // unités : mV, mA, 0,1 °C
pub enum Reg { LoadSwitches, WdgTimeout, WdgCtrl, RadioCfg }
pub enum Line { Payload = 0, Radio = 1, Heater = 2 }      // bit n de LoadSwitches
pub enum ResetCause { PowerOn, Watchdog, Brownout, Commanded, Panic, Unknown }
pub enum HalError { NotPowered, OutOfRange, Busy, Io }
```

- Le firmware n'a **pas d'horloge** : il compte ses propres ticks (perdus au reset).
- Les actionneurs passent uniquement par `Reg::LoadSwitches` (bit à 1 = ligne alimentée).
- Watchdog matériel (dans le sim) : compteur rechargé à `WdgTimeout` par `wdg_kick`, clés alternées
  `0xA5` / `0x5A` (une clé fausse ou répétée ne recharge pas). Expiration → reset de l'OBC.
- NVM : `NVM_SIZE = 256` octets, persiste aux resets. RAM : perdue à **tout** reset.

### 3.1 Interface sim -> firmware

```rust
pub struct Firmware { /* uniquement : ram: [u8; RAM_SIZE] */ }
impl Firmware {
    pub const fn new() -> Self;
    pub fn boot<H: Hal>(&mut self, hal: &mut H);              // efface la RAM, relit la NVM
    pub fn step<H: Hal>(&mut self, hal: &mut H) -> StepReport; // un cycle complet = 1 tick
    pub fn ram_mut(&mut self) -> &mut [u8; RAM_SIZE];          // port d'injection, réservé au sim
}
pub const RAM_MAP: &[Region]; // { name, start, len, kind } : Sched, Mode, Fdir, Proto, Tm, Log, Unused
pub struct StepReport { pub executed_tc: Option<(u32, u8)>, pub mode: Mode, pub kicked: bool }
```

**Règle d'honnêteté mémoire (structurante)** : tout état du firmware qui survit d'un tick au suivant
vit dans l'image `ram` (accès par cellules typées à décalage constant), y compris l'état **non
protégé**. La structure `Firmware` ne contient aucun autre champ. C'est ce qui permet au sim de
basculer n'importe quel bit sans `unsafe`, et d'atteindre aussi le FDIR et l'ordonnanceur.
Les seuils et tables constantes sont des `const` (assimilés à de la ROM, non injectés).
La région `Unused` doit rester < 10 % de `RAM_SIZE`.

`StepReport` est un **instrument** : `executed_tc` sert à l'oracle (critère S5), `mode` et `kicked`
ne servent qu'au débogage et au dashboard. Le verdict de survie n'utilise aucun autre interne.

Séquence d'un tick dans `World::step` :
1. appliquer les pannes dues à ce tick ; 2. physique (orbite, énergie, thermique, latch-up, brownout) ;
3. si l'OBC est alimenté, non figé et hors délai de boot : `firmware.step(hal)` ;
4. watchdog matériel ; resets en attente (`boot` après `BOOT_DELAY`) ;
5. canal radio dans les deux sens (avec pannes de canal) ; 6. `ground` ; 7. mesures de l'oracle.

## 4. Firmware

### 4.1 Ordonnanceur
Exécutif cyclique, ordre fixe = priorité, pas de préemption, pas d'attente bloquante.

| # | Tâche | Critique | Rôle |
|---|---|---|---|
| 1 | `health` | oui | réécrit `WdgTimeout`/`WdgCtrl`/`RadioCfg`, relit `LoadSwitches` et le compare à l'attendu |
| 2 | `power` | oui | lecture et validation des capteurs, machine de modes |
| 3 | `fdir` | oui | surintensité/latch-up, capteurs figés, tempête de resets |
| 4 | `thermal` | oui | thermostat du réchauffeur |
| 5 | `comms_rx` | non | réception, validation et exécution des TC (max 2 trames/tick) |
| 6 | `telemetry` | non | balise périodique, ACK, vidage du journal |
| 7 | `scrub` | non | vote TMR et correction ECC, tourniquet ; image complète en <= 60 s |
| 8 | `payload` | non | charge utile factice |

Invariants : aucune tâche critique ne lit un résultat produit par une tâche non critique du même
cycle. Le `wdg_kick` est émis en fin de cycle **seulement si** les 4 tâches critiques ont posé leur
drapeau d'exécution ce cycle et si le compteur de battement du FDIR a avancé.

### 4.2 Modes

| Mode | Payload | Balise | Réchauffeur | Tâches |
|---|---|---|---|---|
| `Nominal` | ON | 10 s | thermostat | toutes |
| `Eco` | OFF | 30 s | thermostat | toutes sauf `payload` |
| `Survie` | OFF | 60 s | thermostat bas | 1 à 7 |

Le récepteur radio est alimenté dans tous les modes.

Tension validée `V` : A et B d'accord (écart <= 150 mV) → moyenne ; désaccord → la plus basse +
drapeau ; un seul capteur valide → celui-là + drapeau ; aucun → `Survie`.

| Transition | Condition |
|---|---|
| boot → `Survie` | toujours |
| `Survie` → `Eco` | `V >= 7080 mV` pendant 60 s, uptime >= 120 s, pas d'alarme FDIR critique, plafond >= `Eco` |
| `Eco` → `Nominal` | `V >= 7560 mV` pendant 60 s, plafond = `Nominal`, pas de verrou « tempête de resets », ligne Payload non isolée |
| `Nominal` → `Eco` | `V < 7200 mV` pendant 10 s, ou plafond abaissé, ou ligne Payload isolée |
| tout → `Survie` | `V < 6720 mV` pendant 10 s ; aucun capteur de tension valide ; `T_batt` hors [-5, +45] °C ; tempête de resets ; plafond = `Survie` |

Le **plafond** de mode est fixé par TC `SetMode` (défaut `Nominal`, persisté en NVM). Le sol ne peut
qu'abaisser ou relever le plafond ; il ne force jamais un mode au-dessus de ce que l'énergie permet.

### 4.3 Protections

- **TMR** (3 copies + vote, réécriture à la lecture) : mode, plafond, `last_tc_seq`, drapeaux et
  compteurs du FDIR, état d'isolation des lignes, drapeaux de l'ordonnanceur.
- **SECDED Hamming** : journal d'événements et tampons de télémétrie (code (13,8) ou (39,32) au
  choix de `systems`, consigné). Erreur double détectée → donnée invalidée, pas utilisée.
- **NVM** : 3 copies du bloc persistant (`boot_count`, `last_tc_seq`, plafond, isolations), chacune
  avec CRC-32. Lecture : première copie valide, puis réparation des autres. Aucune valide → valeurs
  par défaut + drapeau `NVM_LOST` dans la balise.
- **Registres** : réécrits à chaque cycle par `health` (rafraîchissement systématique).
- **Qui surveille le surveillant** : le watchdog matériel est indépendant du firmware ; son
  rechargement dépend du battement du FDIR ; l'état du FDIR est en TMR ; ses seuils sont en ROM.

### 4.4 FDIR

| Détection | Critère | Réaction |
|---|---|---|
| Latch-up ligne L | `ILine > seuil(L)` sur 3 échantillons consécutifs | coupe L pendant 5 s puis réarme ; 3 récidives en 600 s → L **isolée** (Radio : jamais isolée, cycle seulement) |
| Capteur figé | valeur brute identique pendant 120 s alors que le redondant a varié de >= 30 mV / 1 °C | capteur invalidé, bit dans `sensor_fault_mask` |
| Capteur hors plage / `Err` | immédiat | idem |
| Registre altéré | relecture != attendu | réécriture + compteur |
| Tempête de resets | >= 3 boots avec uptime < 300 s (compteur NVM) | verrou : `Survie` jusqu'à 1 orbite stable (5700 s) ou TC `ClearFdir` |
| Incohérence d'état | vote TMR sans majorité, index hors bornes | `sys_reset` |

**Panique** : un `panic` du firmware (débordement arithmétique, index hors bornes après corruption)
est rattrapé par le sim (`catch_unwind`) et traité comme le ferait un gestionnaire de panique
embarqué : reset immédiat, `ResetCause::Panic`, compté dans les métriques. Ce n'est pas un échec du
harnais.

## 5. Protocole radio : `firmware::proto`

Une seule implémentation, partagée par `firmware` et `ground`. Octets en gros-boutiste.

| Décalage | Taille | Champ |
|---|---|---|
| 0 | 2 | SYNC = `0xAE 0x61` |
| 2 | 1 | TYPE : quartet haut = version (1), quartet bas = 1 TC, 2 ACK, 3 BEACON, 4 EVENTS |
| 3 | 4 | SEQ `u32` (compteur TC côté montant, compteur TM côté descendant) |
| 7 | 1 | LEN (0..=48) |
| 8 | LEN | charge utile |
| 8+LEN | 4 | CRC-32 IEEE (réfléchi, poly `0xEDB88320`, init et xor final `0xFFFFFFFF`) sur les octets `[2, 8+LEN)` |

`MAX_FRAME = 60`. Vecteur de test obligatoire : CRC-32 de `"123456789"` = `0xCBF43926`.

**Validation d'une TC, dans l'ordre** : SYNC, version/type, LEN cohérent avec la longueur reçue, CRC,
puis anti-rejeu : acceptée ssi `last_tc_seq < SEQ <= last_tc_seq + 1024`. À l'acceptation,
`last_tc_seq = SEQ` (TMR + NVM) **avant** exécution. Toute trame rejetée : silence radio (pas d'ACK),
`cnt_tc_rejected += 1`. La balise publie `last_tc_seq`, ce qui permet au sol de se resynchroniser.

| Opcode | TC | Arguments | Effet |
|---|---|---|---|
| `0x01` | `Ping` | — | ACK seulement |
| `0x02` | `SetMode` | mode `u8` | fixe le plafond |
| `0x03` | `Reboot` | — | `sys_reset` après l'ACK |
| `0x04` | `PowerCycle` | ligne `u8` | coupe 5 s puis réarme |
| `0x05` | `ClearFdir` | — | lève isolations et verrou de tempête |
| `0x06` | `DumpEvents` | — | émet le journal (trames EVENTS) |

ACK : `tc_seq u32, opcode u8, result u8` (`0` Ok, `1` BadArg, `2` Refused).

BEACON (28 octets) : `uptime_s u32, boot_count u16, reset_cause u8, mode u8, vbatt_mv u16,
temp_batt_dc i16, last_tc_seq u32, fdir_flags u16, sensor_fault_mask u16, line_state u8,
cnt_ecc_corr u16, cnt_tmr_fix u16, cnt_tc_rejected u16, cnt_latchup u8`.

Hors périmètre, à dire dans le rapport : pas d'authentification ni de chiffrement (un adversaire
actif n'est pas modélisé), pas de code correcteur sur le lien (Reed-Solomon → `docs/later.md`).

## 6. Simulateur : `sim`

```rust
pub struct SimConfig { pub seed: u64, pub duration_s: u32, pub faults: Vec<FaultEvent>,
                       pub params: Params, pub protections: bool }
pub struct FaultEvent { pub t: u32, pub fault: Fault }
impl World {
    pub fn new(cfg: SimConfig) -> World;
    pub fn step(&mut self) -> Snapshot;        // état physique + vue sol, pour le dashboard
    pub fn inject(&mut self, f: Fault);        // injection manuelle (dashboard)
    pub fn queue_cmd(&mut self, c: Command);   // TC manuelle (dashboard)
    pub fn finish(self) -> RunResult;          // verdict + métriques
}
pub fn run(cfg: SimConfig) -> RunResult;
```

Modèles (volontairement simples, valeurs en annexe A) :
- **Orbite** : période fixe, fraction d'éclipse fixe, une passe sol par orbite. Phase initiale tirée
  au hasard par run.
- **Énergie** : `E += (P_solaire - somme des charges alimentées) * dt`, bornée ; tension linéaire en
  état de charge. Coupure matérielle de l'OBC sous `SOC_OFF`, redémarrage à froid au-dessus de `SOC_ON`
  (la charge solaire continue OBC éteint).
- **Thermique** : un nœud (batterie), relaxation vers une température d'environnement soleil/éclipse,
  plus la puissance du réchauffeur.
- **Capteurs** : vérité + bruit gaussien quantifié (indispensable : un capteur sain varie, ce qui rend
  le capteur figé détectable).
- **Canal radio** : transmet les trames entières pendant la passe uniquement ; sans panne, pas de perte.

Module `ground` :
```rust
impl Ground {
    pub fn new(policy: Policy) -> Ground;
    pub fn on_frame(&mut self, t: u32, bytes: &[u8]);               // rejette tout ce qui est invalide
    pub fn poll_tx(&mut self, t: u32, in_view: bool) -> Option<([u8; MAX_FRAME], usize)>;
    pub fn queue(&mut self, c: Command);
    pub fn view(&self) -> &GroundView;  // dernière balise valide, ACK reçus, compteurs de rejets
}
```
Opérateur automatique de campagne : un `Ping` au début de chaque passe puis toutes les 60 s. Il
n'envoie **aucune commande de récupération** : la campagne mesure la récupération autonome.

## 7. Modèle de pannes

Une panne = `(classe, paramètres, t_inj)` entièrement tirée du PRNG du run. Campagne principale :
**une panne par run**. Les doubles pannes sont une campagne à part, rapportée à part.

| ID | Classe | Injection (dans le sim) | Détection / récupération attendue | Prio |
|---|---|---|---|---|
| F1 | `RamSeu` | 1 bit basculé, position uniforme sur toute l'image RAM | vote TMR, SECDED, scrub ; sinon contrôles de plausibilité ou reset | P1 |
| F2 | `RamMbu` | 2 à 4 bits adjacents | SECDED détecte, TMR corrige si une seule copie touchée | P1 |
| F3 | `RegSeu` | 1 bit dans `LoadSwitches`, `WdgTimeout`, `WdgCtrl` ou `RadioCfg` | rafraîchissement par `health` | P1 |
| F4 | `LatchUp` | surintensité sur Payload, Radio ou Heater ; ligne **détruite** si non coupée en `T_SEL` ; ne s'efface que ligne coupée >= 2 s | FDIR surintensité | P1 |
| F5 | `SensorStuck` | capteur figé à sa valeur courante (permanent), ou renvoyant `Err`, ou hors plage | redondance + détection de figeage | P1 |
| F6 | `TcCorrupt` / `TcReplay` / `TcTruncate` | sur la prochaine trame montante : 1 à 8 bits basculés ; réémission d'une trame valide déjà vue ; troncature | CRC, longueur, anti-rejeu | P1 |
| F7 | `WdgSpurious` / `WdgRegHit` | reset intempestif ; bit basculé dans la config du watchdog | redémarrage propre ; rafraîchissement | P1 |
| F8 | `FdirHit` / `SchedHit` | comme F1 mais ciblé sur les régions `Fdir` / `Sched` de `RAM_MAP` | TMR + battement + watchdog | P1 |
| F9 | `CpuHang` | le sim cesse d'appeler `step` (boucle infinie simulée) jusqu'au prochain reset | watchdog matériel | P1 |
| F10 | `BattDeplete` | état de charge forcé à 0–10 % pendant une éclipse | coupure matérielle, redémarrage autonome au soleil | P1 |
| F11 | `NvmCorrupt` | 1 à 8 bits dans l'image NVM | 3 copies + CRC | P1 |
| F12 | `TmCorrupt` | bits basculés sur une trame descendante | rejet par `ground` | P2 |
| F13 | `ObcLatchUp` | surintensité OBC ; le limiteur matériel coupe l'OBC 10 s | redémarrage à froid | P2 |
| D1 | `WdgDead + CpuHang` | double panne : watchdog mort puis blocage | **aucune** : perte attendue, à rapporter telle quelle | P2 |

`t_inj` uniforme dans `[300 s, 6000 s]` (toutes les phases d'orbite sont couvertes par la phase
initiale aléatoire). Pannes de canal F6/F12 : appliquées à la première trame concernée après `t_inj`.

### 7.1 Reproductibilité
- PRNG maison (`splitmix64` pour dériver, `xoshiro256**` pour tirer), aucun appel à l'horloge ni à
  l'OS, pas d'itération sur `HashMap`.
- Graine du run `i` = `splitmix64(graine_campagne XOR i)`. Le résultat d'un run ne dépend ni du
  nombre de fils ni de l'ordre d'exécution.
- Chaque ligne de `runs.csv` contient : `run_id, seed, classe, paramètres, t_inj, verdict, catégories,
  métriques`. `campaign --replay <seed>` rejoue le run à l'identique.
- Test obligatoire : deux campagnes de même graine produisent un `runs.csv` identique octet pour octet.

### 7.2 Limites assumées du modèle (à recopier dans le rapport)
- Les basculements de bits touchent l'image RAM, la NVM et les registres simulés. **Ni la pile, ni les
  variables locales d'un cycle, ni le code, ni le compteur ordinal** ne sont injectés ; `CpuHang` et
  la panique en sont des substituts grossiers.
- Les seuils en `const` sont immunisés par construction.
- Physique à un nœud, orbite idéalisée, pas d'attitude, pas de vieillissement.
- Les taux par classe sont conditionnels à « une panne de cette classe a eu lieu » ; la pondération
  entre classes est arbitraire et ne représente aucun environnement radiatif réel.

## 8. Définition de « le satellite survit » (figée AVANT la campagne)

Un run dure `T_RUN = 3 orbites`. L'oracle vit dans `sim` et ne lit que la **vérité physique** du
simulateur, ce que **le sol a reçu**, et `StepReport.executed_tc`. Jamais l'état interne du firmware.

Le run **survit** si et seulement si les six critères sont vrais :

| | Critère | Mesure |
|---|---|---|
| S1 | Pas de perte matérielle vitale | ligne Radio non détruite, batterie non détruite |
| S2 | Vivant et commandable à la fin | pendant la **dernière passe** : >= 1 balise valide reçue par le sol **et** >= 1 `Ping` acquitté |
| S3 | Énergie | OBC alimenté à `T_RUN` et état de charge réel >= 20 % |
| S4 | Thermique | `T_batt` réelle jamais hors [-10, +50] °C plus de 300 s consécutives |
| S5 | Intégrité des commandes | aucune TC exécutée qui ne soit pas une trame émise par le sol, intacte, exécutée une seule fois ; aucune TM corrompue acceptée par le sol |
| S6 | Stabilité | <= 2 resets pendant la dernière orbite |

Verdict d'un run survivant :
- `OK_NOMINAL` : dernière balise en mode `Nominal`, ligne Payload réellement alimentée, aucune ligne
  détruite ni isolée.
- `OK_DEGRADED` : survit, mais pas nominal à `T_RUN` (mode inférieur, ligne non vitale détruite ou
  isolée, capteur invalidé qui bloque la remontée).

Le rapport donne **les deux taux** : survie (`OK_NOMINAL + OK_DEGRADED`) et retour au nominal.

### 8.1 Catégories d'échec
Toutes les catégories applicables sont loguées ; la catégorie principale est la première dans cet ordre.

| Code | Catégorie | Critère |
|---|---|---|
| E0 | `HARNESS` | panique ou incohérence du simulateur lui-même : run exclu du taux, compté et listé |
| E1 | `VITAL_LOSS` | S1 faux |
| E2 | `THERMAL` | S4 faux |
| E3 | `DEAD` | OBC alimenté mais plus aucun cycle valide en fin de run (figé, non récupéré) |
| E4 | `BOOT_LOOP` | S6 faux |
| E5 | `POWER` | S3 faux |
| E6 | `COMMS_LOST` | firmware actif mais S2 faux |
| E7 | `CMD_INTEGRITY` | S5 faux |
| E8 | `SILENT_LIE` | survit selon S1–S6 mais la dernière balise ment sans le signaler : tension à plus de 200 mV ou température à plus de 5 °C de la vérité sans bit dans `sensor_fault_mask`, ou mode annoncé incohérent avec l'état réel de la ligne Payload depuis plus de 60 s |

`SILENT_LIE` compte comme **échec**.

### 8.2 Métriques par run
verdict, catégories, nombre de resets par cause, délai injection → première balise valide suivante,
délai injection → retour `Nominal` (ou « jamais »), état de charge minimal, `T_batt` min/max, compteurs
de la dernière balise, région RAM touchée.

### 8.3 Règles de mesure (anti-biais)
1. **Témoin sans panne** : 1000 runs sans injection → 100 % `OK_NOMINAL` exigé, sinon la campagne
   n'a pas de sens.
2. **Témoin sans protections** (`SimConfig.protections = false`, feature `unprotected` du firmware :
   vote, ECC, rafraîchissement et FDIR neutralisés) sur les mêmes graines. Si le taux de survie ne
   baisse pas nettement, c'est que les pannes ne mordent pas : le rapport doit le dire.
3. Pannes tombées en région `Unused` : comptées à part ; le taux F1/F2 est donné avec et sans elles.
4. Campagne : >= 20 000 runs au total, >= 1000 par classe P1. Chaque taux est publié avec son
   effectif et un intervalle de Wilson à 95 %. Jamais de taux global sans le détail par classe.
5. Les paramètres de l'annexe A et cette section sont gelés avant le premier run de campagne ; on ne
   retouche ni les seuils ni l'oracle après avoir vu les résultats.

Sorties : `results/<nom>/runs.csv`, `results/<nom>/summary.json`.
CLI : `cargo run --release -p sim --bin campaign -- --seed S --runs N [--class C] [--unprotected] [--replay SEED] --out results/<nom>`.

## 9. Dashboard

Binaire `dashboard`, écoute `127.0.0.1:8080`, HTTP/1.1 minimal sur `std::net`, une page statique
embarquée (`include_str!`), pas de framework, pas de WebSocket (interrogation toutes les secondes).
Un fil fait tourner un `World` en temps accéléré ; l'état partagé est un `Snapshot` sous `Mutex`.

| Route | Rôle |
|---|---|
| `GET /` | page : mode, orbite/éclipse, batterie, température, lignes, compteurs, dernières balises, journal |
| `GET /api/state` | `Snapshot` en JSON |
| `GET /api/campaign` | contenu de `results/<nom>/summary.json` |
| `POST /api/fault` | injecte une panne (classe + paramètres) |
| `POST /api/cmd` | met une TC en file |

## 10. Hors périmètre
Matériel réel et compilation croisée, attitude/ADCS, orbite réaliste (SGP4), authentification radio,
Reed-Solomon, mise à jour du firmware en vol, RTOS préemptif, modèle radiatif physique.

---

## Annexe A — valeurs numériques (calibrables par `docs/decisions.md` jusqu'au gel de campagne)

| Paramètre | Valeur |
|---|---|
| Tick | 1 s |
| Orbite / éclipse / passe sol | 5700 s / 2100 s / 600 s |
| `T_RUN` | 17 100 s |
| Batterie | 10 Wh ; `V = 6000 mV + 24 mV x SoC(%)` ; SoC initial tiré dans [60, 100] % |
| `SOC_OFF` / `SOC_ON` | 5 % (6120 mV) / 15 % (6360 mV) |
| Puissance solaire au soleil | 4,0 W |
| Charges | OBC 0,4 W ; récepteur 0,2 W ; émission 3,0 W (1 s par trame) ; Payload 1,2 W ; Heater 1,5 W |
| Watchdog | délai 10 s ; `BOOT_DELAY` 5 s |
| Latch-up | surintensité +300 à +800 mA ; seuil = courant nominal max + 150 mA ; `T_SEL` = 30 s |
| Thermostat | ON sous +2 °C, OFF au-dessus de +8 °C (`Survie` : 0 / +5 °C) |
| Bruit capteurs | écart-type 5 mV, 5 mA, 0,2 °C |
| `RAM_SIZE` / `NVM_SIZE` | 1024 / 256 octets |

Contraintes de calibration (à vérifier par test, pas à supposer) :
- bilan d'énergie positif sur une orbite en `Nominal` sans panne (ordre de grandeur : 3,3 Wh
  consommés pour 4,0 Wh produits) ;
- sans réchauffeur, `T_batt` passe sous -10 °C en éclipse en moins de 1500 s ; avec thermostat sain,
  elle reste >= 0 °C ;
- après `BattDeplete`, le redémarrage à froid intervient dans l'orbite qui suit.
