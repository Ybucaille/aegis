# T-007 Ground station : décodage TM, opérateur automatique, file de TC
Agent: builder   Modèle: Sonnet   Dépend de: T-002
Entrée: `docs/SPEC.md` §5, §6 (module `ground`) ; `firmware/src/proto.rs`
Sortie attendue: `ground/src/lib.rs` (+ modules) : `Ground::{new, on_frame, poll_tx, queue, view}`, `Policy`
(`Auto` = `Ping` au début de chaque passe puis toutes les 60 s, AUCUNE commande de récupération ; `Manual` =
file seulement), `GroundView` (dernière balise valide, ACK reçus avec date, compteurs de rejets par motif,
historique borné des balises). Numéro de séquence TC croissant, resynchronisé sur `last_tc_seq` de la balise.
Utilise exclusivement `firmware::proto` pour coder/décoder (aucune réimplémentation du CRC ni du format).
Lot J1-B, parallèle avec T-005 et T-006 : n'écrit que dans `ground/`.
Critère de fin (vérifiable): `cargo test -p ground` → vert, avec au minimum : balise valide → `view` mise à jour ;
trame avec 1 bit basculé, tronquée, de type inconnu → rejetée et comptée, `view` inchangée ; hors visibilité →
`poll_tx` renvoie `None` ; en visibilité → un `Ping` à t0 puis un toutes les 60 s ; resynchronisation du SEQ après
une balise annonçant un `last_tc_seq` plus grand ; aucun `panic` sur 10 000 tampons aléatoires. `./scripts/ci.sh` → `CI OK`.
## Compte rendu (rempli par l'agent)
