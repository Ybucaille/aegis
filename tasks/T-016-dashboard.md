# T-016 Dashboard web sobre
Agent: builder   Modèle: Sonnet   Dépend de: T-014
Entrée: `docs/SPEC.md` §9 ; API publique de `sim` (`World`, `Snapshot`, `Fault`) et de `ground` (`Command`)
Sortie attendue: `dashboard/src/main.rs` (+ modules : `http.rs` HTTP/1.1 minimal sur `std::net`, `json.rs`),
`dashboard/static/index.html` (embarquée par `include_str!`, sans framework ni ressource externe), routes
`GET /`, `GET /api/state`, `GET /api/campaign`, `POST /api/fault`, `POST /api/cmd` ; un fil fait tourner un
`World` en temps accéléré, `Snapshot` sous `Mutex` ; écoute `127.0.0.1:8080` uniquement (option `--port`).
Sobre : une page, pas d'animation. Aucune modification de `sim/`, `ground/`, `firmware/` : si un champ manque
dans `Snapshot`, le demander au manager.
Lot J2-B, parallèle avec T-015 et T-012 : n'écrit que dans `dashboard/`. PREMIÈRE tâche à sacrifier si le jour 2
déborde (le rapport chiffré prime) : dans ce cas livrer `GET /` + `GET /api/state` seulement et le dire.
Critère de fin (vérifiable): `cargo test -p dashboard` → vert (analyse de requête, requête mal formée → 400,
route inconnue → 404, sérialisation JSON) ; puis `cargo run -p dashboard -- --port 8080 &` et
`curl -s localhost:8080/api/state | grep -c '"mode"'` → 1 ; `curl -s -o /dev/null -w '%{http_code}' localhost:8080/` → 200 ;
`curl -s -X POST localhost:8080/api/cmd -d '{"cmd":"Ping"}'` → 200 puis un ACK visible dans `/api/state` à la
passe suivante. `./scripts/ci.sh` → `CI OK`.
## Compte rendu (rempli par l'agent)
Fait : `dashboard/src/{main,http,json}.rs`, `dashboard/static/index.html`. Routes GET /, /api/state, /api/campaign
(`?name=` ou dernier dossier de `results/` ; nom restreint à [A-Za-z0-9_-]), POST /api/fault ({"class":"F1".."D1","seed":N
optionnel}), POST /api/cmd ({"cmd":"Ping|Reboot|ClearFdir|DumpEvents|SetMode|PowerCycle","arg":n}). 127.0.0.1 seulement,
options --port --seed --speed (défaut 120 ticks/s). Pannes : tirées par `sim::fault::draw(classe, graine)` (graine par défaut
= graine du monde + compteur), injectées immédiatement (dates du tirage ignorées), graine + paramètres loggués sur stderr et
dans le journal de /api/state. Aucune modification hors dashboard/.
Vérifié : `cargo fmt -p dashboard -- --check` OK ; `cargo clippy -p dashboard --all-targets -- -D warnings` OK ;
`cargo test -p dashboard` 17 passés (400/404/405, analyse de requête, JSON). Test réel (--port 8099 --speed 3000) :
`/api/state | grep -c '"mode"'` = 1 ; `GET /` = 200 ; POST /api/cmd Ping = 200 puis ACK Ping visibles dans "acks" ; route
inconnue = 404 ; POST /api/fault F1 seed=5 = 200 (RamSeu byte=155 bit=3). Non lancé : ./scripts/ci.sh (workspace partagé en cours
de modification par les autres agents).
Écarts / limites : une passe sol n'a lieu que 600 s sur 5700 s simulées, donc l'ACK d'un Ping n'apparaît qu'à la passe suivante
(à 120 ticks/s : jusqu'à ~45 s réelles). Page non testée dans un navigateur (JS relu seulement). Pas de lecture d'événements
EVENTS (DumpEvents) affichés, seulement leur ACK. Biais : la panne manuelle ignore la date tirée, donc pas de contrôle de la
fenêtre T_INJ ; le monde dashboard n'a pas d'oracle/verdict affiché.
