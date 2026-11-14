# T-017 Tests de masse et fuzzing maison du protocole (firmware + ground)
Agent: tester   Modèle: Haiku   Dépend de: T-009
Entrée: `docs/SPEC.md` §5, §8 (S5) ; `firmware/src/proto.rs`, `ground/src/lib.rs`, `sim/src/prng.rs`
Sortie attendue: `firmware/tests/fuzz_proto.rs`, `ground/tests/fuzz_ground.rs` (tests d'intégration seulement,
aucun fichier sous `src/`). Fuzzing sans dépendance externe : générateur déterministe à graine fixe recopié dans
`tests/` (pas de `cargo-fuzz`, pas de `proptest`). Cas : (1) 1 000 000 de tampons aléatoires de 0 à 60 octets →
le décodeur ne panique jamais ; (2) trames valides avec 1 à 8 bits basculés → jamais acceptées avec un contenu
différent de l'original (compter et afficher les collisions de CRC éventuelles, ne pas les masquer) ; (3) toute
troncature d'une trame valide → rejet ; (4) LEN incohérent → rejet ; (5) côté `ground` : mêmes mutations sur
BEACON/ACK → `view` inchangée.
Le tester ne corrige pas le code testé : un défaut trouvé = test laissé rouge + graine dans le compte rendu.
Lot J2-A, parallèle avec T-010 et T-013 : n'écrit que dans `firmware/tests/` et `ground/tests/`.
Critère de fin (vérifiable): `cargo test --release -p firmware --test fuzz_proto -- --nocapture && cargo test --release -p ground --test fuzz_ground -- --nocapture`
→ vert en < 120 s, la sortie affiche pour chaque cas `cas=N essais=… acceptés_à_tort=0`. `./scripts/ci.sh` → `CI OK`.
Ajout PRE-MORTEM (R7) : cas (6) encodeur de référence INDÉPENDANT écrit dans `tests/` à partir de `docs/SPEC.md` §5
seule (sans recopier `proto.rs`, CRC bit à bit sans table) ; test différentiel : 100 000 trames aléatoires
(type, SEQ, LEN, charge utile) → octets identiques à ceux de `firmware::proto`, et les trois vecteurs de l'annexe
de `docs/risks.md` retrouvés par les deux. Un écart = test laissé rouge + trame en cause dans le compte rendu.
## Compte rendu (rempli par l'agent)
Fait (tester, Haiku ; compte rendu reporté par le manager car l'agent n'écrivait que dans */tests/).
- Fichiers : firmware/tests/fuzz_proto.rs (cas 1, 1b, 2, 3, 4, 6), ground/tests/fuzz_ground.rs (cas 5a, 5b, 5c). Aucun src/ touché.
- 5 601 219 entrées fuzzées + 100 000 trames en différentiel ; 0 divergence, 0 accepté à tort, 0 collision CRC (1 à 8 bits, 200 000 essais par k).
- Encodeur de référence écrit depuis SPEC §5 seule ; 3/3 vecteurs dorés retrouvés par référence et proto.
- Graine : AEGIS_FUZZ_SEED (défaut 0x00AE_6100_0000_0017), affichée sur chaque ligne `cas=…`, rejeu vérifié.
- `cargo test --release -p firmware --test fuzz_proto` vert ; `cargo test --release -p ground --test fuzz_ground` vert ; ci.sh → CI OK (vérifié par le manager).
