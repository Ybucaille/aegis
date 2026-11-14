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
