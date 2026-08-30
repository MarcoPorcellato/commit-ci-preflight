# Prompt da incollare in Codex dopo il riavvio

Riprendi il programma Clean Architecture e Capability Packs di
Commit-CI-Preflight in modalità evidence-first.

Prima di qualsiasi azione, leggi integralmente:

1. `/Users/marco1/.codex/AGENTS.md`
2. `/Users/marco1/.codex/CCP_USAGE.md`
3. `/Users/marco1/.codex/handoffs/commit-ci-preflight/2026-08-30-capability-packs-restart/RECOVERY_MANIFEST.md`
4. `/Users/marco1/.codex/handoffs/commit-ci-preflight/2026-08-30-capability-packs-restart/CCP_CAPABILITY_PACKS_RESTART_HANDOFF.md`
5. la specifica, il piano M2, il progress e il goal copiati nella stessa directory.

Usa Superpowers. Delega a GPT-5.6 Luna o Codex Spark soltanto inventari,
documentazione, test deterministici e review bounded; conserva centralmente
architettura, sicurezza, integrazione, qualificazione, release e decisione
finale. Tutti i comandi shell devono iniziare con `rtk`.

Esegui esclusivamente l'audit read-only post-riavvio descritto nel handoff:

- verifica ogni SHA-256 nel manifest e `git bundle verify`;
- verifica branch/ref/HEAD/tree/status/remoti e registrazioni worktree senza
  reset, stash, clean, prune, repair o cancellazioni;
- non affidarti a `/private/tmp`: se il vecchio worktree è assente o diverge,
  proponi una clone locale dal bundle verificato e fermati prima di crearla;
- verifica percorso, SHA-256 e versione del binario CCP stabile;
- verifica admission, risorse, recovery, Docker e processi soltanto read-only;
- preserva integralmente il journal `4911b8ac...`;
- non fare fetch o interrogazioni GitHub senza nuova autorizzazione.

Poi restituisci Facts / Unknowns / proposta / GO o NO-GO e fermati al gate
esterno. Non avviare M2, build, test completi, CCP heavy, Docker workload,
cleanup, push, PR, merge, installazione, tag o release.

Se tutti gli anchor locali sono integri, il passo successivo da autorizzare è
il fetch read-only, seguito solo su base riconciliata dal push non-forzato della
branch, draft PR e hosted `Rust CI` exact-head. M2 Task 1 può iniziare soltanto
dopo il successo hosted esatto.
