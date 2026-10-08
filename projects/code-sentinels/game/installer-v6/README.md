# V6 Preview installer packaging

This packages the already marked `58b79154` V6 preview. It does not change the game's source, assets, native executable, candidate flag or final-release acceptance requirements. The installer itself is a separate, unsigned preview artifact.

`build_preview_installer.py` accepts a frozen candidate directory and a new work directory. It verifies every original file against `v6-candidate.json`, preserves the marker and original QA/licensing sources, and copies only that allowlist. Save files, logs and private signing-application records outside the allowlist are not copied. The generated `installed-files.json` also records the separate launcher, instructions and installer-tool notice.

The generated Inno Setup script installs for the current user, creates ordinary desktop/start-menu shortcuts, and provides uninstall registration. It does not elevate privileges, change firewall or security policies, install system DLLs, enable a compiler, or claim a code-signing grant. Its uninstall file list does not include player-created saves/replays/logs.

The bundled launcher reopens a healthy service only when its reported root and process identity match the current installation. A cold launch owns its child bridge and requests normal shutdown through the bridge's existing stdin control. Its file locks are installation-local; only verified dead, recognized records may be quarantined during recovery. Startup errors remain visible in the command window and local log. `--no-open` is for controlled installation checks and suppresses only default-browser opening.

The build outputs must retain their own compiler, source, artifact hash and installation-check receipts. A passing local install/launch/uninstall check does not constitute the outstanding full game, balance, performance or LAN acceptance, nor guarantee compatibility with another machine's CPU/GPU or application-control policy. Public redistribution rights for third-party artwork remain an independent review item.
