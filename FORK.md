# Fork test channel (Kartudev/Ferrocord)

Your installed Ferrocord updates from **your fork's** test builds instead of FerroApps releases.

## What's what

| Piece | Where | Fork-only? |
|---|---|---|
| `FERRO_UPDATE_REPO` (build-time repo override for the updater) | branch `claude/project-thread-afdth6` (1 commit on `development`) | **Upstream-worthy**: open it as a PR to FerroApps/Ferrocord `development` if you like |
| `.github/fork/update-repo-override.patch` (same commit as a patch) | fork branch `fork` | Fork-only. The build applies it while upstream lacks the override; delete it once upstream merges it |
| `.github/workflows/test-release.yml` | fork branch `fork` | Fork-only |
| `.github/workflows/fork-sync.yml` | fork branch `fork` | Fork-only |

`main` / `development` in the fork stay byte-identical to upstream (fast-forward only), so the
workflows live on a separate branch `fork`, which must be the fork's **default branch**
(GitHub only runs scheduled workflows from the default branch).

## One-time setup

1. Branch `fork` (development + these fork files): already pushed.
2. Settings → General → Default branch → `fork`.
3. Actions tab → "I understand my workflows, go ahead and enable them".
4. Settings → Secrets and variables → Actions → New secret `SYNC_TOKEN`: a fine-grained PAT
   limited to Kartudev/Ferrocord with **Contents: read/write** and **Workflows: read/write**
   (GITHUB_TOKEN can't push upstream commits that touch `.github/workflows`).
5. Actions → "Fork sync" → Run workflow (first run also disables upstream's `Version` and
   `Release` workflows in the fork so syncing never bumps versions or publishes fake official releases).
6. Actions → "Test release" → Run workflow (ref: `development` or any branch) → wait ~15 min.
7. Install from your fork once.
   **Linux** (replaces the current install, same paths):
   ```sh
   curl -fsSL https://raw.githubusercontent.com/FerroApps/Ferrocord/main/install.sh \
     | FERRO_RELEASE_API=https://api.github.com/repos/Kartudev/Ferrocord/releases/latest sh
   ```
   **Windows 11** (PowerShell; no env var needed, the fork's repo is baked into the build).
   Extract into a folder you can write to (the updater only self-updates writable zip folders):
   ```powershell
   $rel  = Invoke-RestMethod https://api.github.com/repos/Kartudev/Ferrocord/releases/latest
   $zip  = $rel.assets | Where-Object name -like '*-windows-x86_64.zip'
   $sum  = $rel.assets | Where-Object name -like '*-windows-x86_64.zip.sha256'
   $dir  = "$env:LOCALAPPDATA\Ferrocord-test"
   New-Item -ItemType Directory -Force $dir | Out-Null
   Invoke-WebRequest $zip.browser_download_url -OutFile "$env:TEMP\$($zip.name)"
   Invoke-WebRequest $sum.browser_download_url -OutFile "$env:TEMP\$($sum.name)"
   $want = ((Get-Content "$env:TEMP\$($sum.name)" -Raw) -split '\s+')[0]
   $got  = (Get-FileHash "$env:TEMP\$($zip.name)" -Algorithm SHA256).Hash.ToLower()
   if ($want -ne $got) { throw "checksum mismatch" }
   Expand-Archive "$env:TEMP\$($zip.name)" $dir -Force
   & (Get-ChildItem $dir -Recurse -Filter ferrocord.exe | Select-Object -First 1).FullName
   ```
   (Pin `ferrocord.exe` from that folder to Start/taskbar. Your official install stays wherever it was.)

   From then on the app's own updater checks your fork (at start + every 6 h, or Settings → Updates → Check).

## Daily use

- Merge what you want to try into branch `testing` and push: within ~30 min a test release is built.
- Or Actions → Test release → Run workflow with any branch.
- Versions look like `0.19.7` (= upstream 0.19.0, run 7) / `0.19.1012` (= 0.19.1, run 12); always increasing.
- Each test release has the Linux tarball and the Windows zip (+ `.sha256`); it is published only once both are attached.
- Only the newest 10 test releases are kept.
- Back to official builds: rerun install.sh without `FERRO_RELEASE_API` (Windows: use the official zip)
  (an official build of the same x.y may be "older" than a test build, so reinstall, don't wait for the updater).

