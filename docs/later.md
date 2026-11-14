# Idées reportées (gel de périmètre)
- (T-019) Campagne d'accumulation : k SEU RAM par run (k = 2, 4, 8…) pour que F1/F2/F8 discriminent protégé / non protégé. Changement de SPEC §7.
- (T-019) F11 suivi d'un reset forcé (la NVM n'est relue qu'au boot) ; `WdgRegHit` suivi d'un `CpuHang`. Pannes composées, changement de SPEC §7.
- (T-019) Témoin non protégé à une seule copie (sans la disposition TMR), pour ne pas diluer les SEU par 3.
- (T-019) `campaign --help`.
