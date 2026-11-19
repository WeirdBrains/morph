# Handoff

Updated 2026-11-15. This file is the resume point for moving Morph's desktop shell to
GPUI. **Phases 1 to 6 are done and pushed to `main`** (phase 6: native Wayland): pre-releases `v0.2.0-alpha.1`
(https://github.com/WeirdBrains/morph/releases/tag/v0.2.0-alpha.1), `v0.2.0-alpha.2`
(https://github.com/WeirdBrains/morph/releases/tag/v0.2.0-alpha.2), `v0.2.0-alpha.3`
(https://github.com/WeirdBrains/morph/releases/tag/v0.2.0-alpha.3), `v0.2.0-alpha.4`
(https://github.com/WeirdBrains/morph/releases/tag/v0.2.0-alpha.4), `v0.2.0-alpha.5`
(https://github.com/WeirdBrains/morph/releases/tag/v0.2.0-alpha.5), `v0.2.0-alpha.6`
(https://github.com/WeirdBrains/morph/releases/tag/v0.2.0-alpha.6) and `v0.2.0-alpha.7`
(https://github.com/WeirdBrains/morph/releases/tag/v0.2.0-alpha.7, fixes for alpha.6 on
m3talsmith's GPU), each with a
`morph-bin` recipe for TAUR. m3talsmith installs with
`yay --aururl https://pkg.taur.weirdbrains.com -S morph-bin`. m3talsmith said to keep
going through the phases without waiting for him. Phase 7 (macOS and Windows
builds, `v0.2.0-alpha.8`) is below.

## Active work (who owns what)

Two agents work on Morph. Before editing an area someone else owns, tell them first. Only
Morph Lead Dev cuts releases and tags or touches version, release.toml or packaging files.
Update this table when you start or finish an area.

| Area | Owner | Status (2026-11-18) |
|---|---|---|
| Networking: weirdnet-net peers and relays, Grindvakt DDNS, `weirdnet-morph://` and the scheme registry | Morph Lead Dev | v0.3, not started |
| Sync and security: weirdnet-sync 0.3.0 key rotation and its Morph wiring, Linked devices | Morph Lead Dev | in progress |
| Device linking crate (2026-11-19) | Morph Lead Dev | `weirdnet-device-link` comes from crates.io `=0.3.0` (in-tree copy removed); link protocol `/weirdnet/link/2` |
| Password saver: backend and storage | Morph Lead Dev | v0.3, in progress (storage landed 9ffd5ce, 92afbec, 904137b; remaining: OsAuth backend, page capture/fill with origin matching, Settings > Privacy toggle and saved-logins list; sync waits on weirdnet-sync) |
| Profile lock: a profile with an identity asks for its password to open (on by default, off in Settings > Privacy and site data with the password, policy `require_profile_password`), sealed `settings.json`/`tabs.json` (`morph-core/src/profile_files.rs`, `profile_lock.rs`), the gate and password change (`morph-services/src/profile_lock.rs`, `morph-identity` `change_password`), lock screen and Lock profile (`morph-desktop/src/lock_view.rs`, `shell.rs`, `settings_view/lock.rs`), [docs/profile-lock.md](profile-lock.md). Touch point in Morph Dev's `mcp_host.rs`: `Hooks::locked` (no server runs for a locked profile; the watcher reloads on lock and unlock) | Morph Lead Dev | v0.3, in progress (on main 1d4b65f..bf6cb6e), unreleased. Open for Morph Dev: seal `companions/`, `ai/policy.json` and the MCP host's `mcp-*.local.json` under the profile file key (`morph_core::profile_files::{read, write}`). Not yet: auto-lock after idle, OS auth unlock, the Flutter gate with biometrics |
| 0.2.x patches: omnibox suggestions, transparent popups, multi-window follow-ups, downloads | Morph Lead Dev | in progress |
| Releases and packaging | Morph Lead Dev | v0.2.1 in progress |
| Agent work: new `weirdnet-agent` crate (agent loop, tools, approvals, trace), `morph-ai`, `morph-companion`, `morph-ui` `Slot::Companion`, projects. Design: [Companions Design v1](https://cursor.com/canvas/store-e1e5cb1f-1e04-4388-8265-5c11c09b3f62/2ca47086-6c82-43a2-ba40-cea34404b597). Branch `main` | Morph Dev | v0.3 (moved from v0.4), in progress: `weirdnet-agent` 0.1.0 is on crates.io and pinned `=0.1.0`; `morph-ai` and `morph-companion` scaffolds are on main with tests. Companion panel (`Slot::Companion`, `ToggleCompanions`, ctrl-shift-k) is on main. Next: real browser tools and research host once Lead Dev sends the bridge API; model provider is `morph_ai::LocalModels` over `weirdnet-model` 0.1.0 (crates.io): consent-gated Gemma 4 download with progress in the panel, llama.cpp CPU on desktop; no GPU feature yet and no real-model end-to-end run yet (no model cached) |
| Companion panel in shared chrome: `Slot::Companion` and `Action::ToggleCompanions` in `morph-ui` (`layout.rs`, `actions.rs`, `component.rs`), and the slot render, request and action dispatch in `morph-desktop/src/shell.rs` plus a new `companion_view.rs`. Not touching the omnibox, the `>` prefix, `suggest.rs` or `page_bridge` | Morph Dev | Slot and panel done on main (see commits); per-site grants UI and real tools wait on the bridge API |
| Agents settings area (Settings > Agents: companions, models, per-site grants, Always allow, research provider, MCP placeholder). Touches the shared settings shell `morph-desktop/src/settings_view.rs` minimally: one `Section::Agents`, one `Item::DefaultCompanion`, a `mod agents;` child module (`settings_view/agents.rs`) holding all of the area, and a `ShellRequest::AgentsChanged` that `shell.rs` forwards to the Companion panel. Also the Stop button on skipped-approval notices in `companion_view.rs`. Nothing in `morph-core` or other sections changes | Morph Dev | done (pushed; MCP toggle is a "coming soon" placeholder, research provider stored but not wired) |
| Design for all v0.3 screens | Morph Dev | in progress |
| Page bridge for hosts: `BridgeContext`, host channel and anti-forgery (`morph-webview/src/bridge.rs`), page reads (`page_read.rs`), host pages (`morph-browser/src/host_pages.rs`), [docs/bridge-api.md](bridge-api.md). Not touching `morph-ai`, `morph-companion`, `Slot::Companion` or Settings > Agents | Morph Lead Dev | done on main 2026-11-18: Linux uses a script world; macOS/Windows token-only (see the doc's per-platform table); mobile not wired |
| Cross-device agent work, pulled into v0.3 (m3talsmith, 2026-11-18): sync of companions, grants, chats, MCP configs and journals (`morph-services/src/companion_sync.rs`; `morph-core/src/companion_sync.rs` holds only the type names, so `morph-core` does not depend on `morph-ai`; sync types `morph-companions`, `morph-grants`, `morph-chats`, `morph-mcp`, `morph-mcp-secrets`, `morph-journal`). Every record goes through `SyncState::write`/`read` (weirdnet-sync records, `companion_sync::configure` the same on every device): chat turns sync whole, chunked past the crate's default `inline_max`, with a clipped fallback (`fallback_turn`, sized from `Limits`) only for older peers; MCP secrets are the sealed `morph-mcp-secrets` type (`k/<server>`, all writes and deletes sealed, empty fallback, refused with `no-epoch-key` before the first rotation and retried each pass after it; values written back exactly, so the 4d37d50 approval fingerprint matches where they came from sync; the snapshot keeps names and a keyed BLAKE3 digest only (key derived from the head epoch key for `morph/mcp-secret-digest/v1`, so a rotation changes it; names only before the first rotation; an older unkeyed digest is dropped on load, so that secret is written once more); sync reads and writes them only through the `LocalSecrets` the MCP host shares, `share_secrets`/`secrets_for`, never the file); a companion that vanishes from the roster leaves the MCP servers' scopes (`morph_ai::forget_vanished_companions`, run by the `mcp_host` watcher on each `applied_revision` or lock change); revoking a device voids its grants and its Node companions; Node companions and inference offload routing (`morph-ai` `remote`: `NodeRouter`, `LinkHandler`, `route_companion`, `offload_plan`; implemented on Lead Dev's `DeviceCaller` in `morph-services/src/node_router.rs`, framed links on `open_stream`, sessions on `accept_streams`; the `weirdnet-agent` protocol glue is only `node_router::protocols`); saved chats (fence-aware clipping, `morph_ai::clip_markdown`), Save to Notes / Save to journal and MCP configs (`morph-ai` `chats`, `journal`, `mcp`; `morph-companion` `chat_turn` and `journal_entry` turn a finished task into them); the generic protocols in `weirdnet-agent` `remote::infer` and `remote::task` (on its main, unreleased). Touch points in Lead Dev's sync code, kept minimal: `morph-core/src/sync.rs` (the five types added to `DEFAULT_TYPES` and `SUPPORTED_TYPES`, previous defaults added to `OLD_DEFAULTS`, `DEFAULTS_VERSION` 5) and `morph-services/src/sync.rs` (`companion_sync::configure` in the live and offline state builders; `tick` calls `companion_sync::backfill_with` beside the settings backfill, retries a type's missing chunks with `pull_chunks`, and calls `materialize_with` after pulls or a roster revocation, both with `secrets_for`; `revoke_device` calls `companion_sync::after_revoke`; the two-homes relay test checks companion records and revoke) | Morph Dev | records, merge, revoke and routing done with tests. **Progress (restart here, 2026-11-19):** keyed digests on main: `profile_files::derive_key` (ae583b6, a per-purpose HKDF key from the unlocked file key; added to Lead Dev's file, no public derive existed), keyed snapshot digests (db82d22), and keyed approval fingerprints `k1:` under `morph/mcp-approval/v1` via `profile_io::derive_key` in `mcp_host::load_approvals` (d2092a0; older plain approvals ask once; a profile without an identity keeps the plain SHA-256 form). Earlier, all three slices on main: scope cleanup on a synced companion removal (ac1f770), chunked chats (340a17c, e83feb8, ed9155f, 0a4ae15), sealed MCP secrets on `FileIo`/`LocalSecrets` (9203dd2, a828808, 1708813, and 623cf95, which fixes 1708813's deadlock: a locked profile is caught by its `FileIo` reads, never a lock-status call inside `library_db::with`), on weirdnet-sync 0.5.0; morph-services 74 tests green. Open: MCP secrets sync only while the MCP host has the profile's `LocalSecrets` open (`mcp_host::get`), and dropping one secret name means `remove_server` then setting the rest (the trait has no per-name remove). The Companion panel reloads its roster and policy when `companion_sync::applied()` moves (any merge that wrote files, the rebuild on revocations included) and saves the policy by rebase (`morph_ai::rebase_shared`). `weirdnet-model` 0.2.1 (its `agent` feature on `weirdnet-agent` 0.2) is published; Morph is on weirdnet-agent =0.2.0, weirdnet-mcp =0.3.0 and weirdnet-model =0.2.1 (pins ad263ce), with the `remote::infer` / `remote::task` wiring in `node_router::protocols` and loopback tests (043d638, lock 64b211d), all pushed 2026-11-19. The Windows CI clippy failure on 0a4ae15 (`manager_with` unused in `mcp_host` tests) is fixed by aae4982. `identity::tests::a_backup_restores_into_another_profile_and_words_into_a_third` (Lead Dev's) fails under heavy load and passes alone. The panel has a `DeviceRouter` (`companion_devices.rs`) and devices exchange capabilities on the `morph-device` app; this desktop says it serves no models until the infer service runs. Not yet: panel UI for Save to Notes / journal, saved chats in the panel, Node companion picker in Agents settings |
| Connected MCP servers (m3talsmith, 2026-11-18): `weirdnet-mcp =0.2.0` (workspace dep, `morph-ai` feature `mcp`); `morph-ai` `mcp_runtime.rs` (`McpHub`: one `McpSource` per enabled, approved server with a slug prefix, tier floor Research, per-tool switches from `tools_off`, a status per server) and `mcp_local.rs` (per-machine approvals `mcp-approved.local.json`, fingerprint of command, args and env names only); `morph-companion` `Sessions::set_tools`; `morph-desktop` `mcp_host.rs` (`McpManager` with its own runtime, `LocalSecrets`: `mcp-secrets.local.json`, mode 0600, beside the profile and outside `companions/`, implements `McpSecrets`) and `settings_view/servers.rs` (Connected servers list in Settings > Agents above the Morph MCP placeholder: add, edit, remove, eye toggles, local-command warning and Review and allow before first start, connect and list errors). Approval is a SHA-256 over program, args (or URL) and every env or header value, secret values hashed in, so any synced or local change asks again (HTTP servers ask first too); `morph_services::sync::applied_revision` is bumped on every applied pull, arriving revocation and local revoke, and the MCP host re-checks every server on a change (Lead Dev's `mcp_may_start_in` pre-connect check unchanged, a revoked writer's server stops at once); `morph_ai::remove_companion` removes a companion and takes its id out of server scopes (a server only for it ends up for nobody); no UI removes companions yet, call it from there. Unsupported CPU: `morph_ai::cpu_limit()` (from `weirdnet_model::cpu::check`; `MORPH_FORCE_UNSUPPORTED_CPU=avx2,fma` forces it) makes `LocalModels` report `ModelStatus::CpuUnsupported`, refuse to open or download, hide the model chips in Settings > Agents and show the reason plus an offload choice (saved in `AgentSettings.offload`; not routed yet). Configs use the synced `McpConfigs` types unchanged; `companions/mcp.json` never holds values | Morph Dev | done with tests. A server applies to all companions or one (`McpServer::companions`, optional and synced; `McpHub::registry_for`, `Sessions::set_tools_for`; picker in the edit form). Waiting on: Lead Dev's keystore to implement `McpSecrets` for MCP values (then swap `LocalSecrets` in `mcp_host::get`). Not yet: a secret typed into a plain-looking field is moved only when its name looks secret; the args field takes shell-style quoting (a backslash escapes only a space, quote or backslash, so Windows paths work) Profile lock (m3talsmith, 2026-11-19): `companions/*` (incl. `exports/`, `tool-log.jsonl`) and every `mcp-*.local.json` are sealed under the profile file key: `morph-ai` `FileIo` seam (`FileStore::with_io`, `FileSecrets::open_with`, `McpApprovals::{load_with,save_with}`, `StoreError::Locked`); `morph-services::profile_io` (`file_store`, `io_beside`, `seal_existing` called after each unlock in `library.rs`); `LocalSecrets` wraps `FileSecrets`. Locked: reads empty/`Locked`, nothing written, secrets reload after unlock. Lead Dev key code untouched. |
| Device identity for linked-device calls: `morph-services/src/device_calls.rs` (`DeviceCaller` on the sync node's key and cert, `Roster::check_caller`, `serve`, `accept_streams`, `call`, `open_stream`), [docs/net-app-caller-identity.md](net-app-caller-identity.md); `NetAppCaller` stays anonymous. Review of the companion sync touch points: chat clipping and revocation-only passes fixed in `companion_sync.rs` and `tick` | Morph Lead Dev | done (2026-11-18). Morph pins `weirdnet-sync` =0.4.0 and `weirdnet-net` =0.7.0 and uses `Roster::check_caller` (local copy dropped, 2026-11-18) |
| Revoked writers in sync: late ops of a revoked device void in every type (`morph-core/src/sync.rs` `late_from_revoked`, bookmarks, `history_sync.rs`, `settings_sync.rs`); every op of a revoked writer void for companions, grants and MCP (`companion_sync.rs` `SENSITIVE`, `mcp_writers`, `mcp_writer_trusted`), with fallback to the latest valid entry; services rematerialize on load, revoke-set change and local revoke (`apply_revocations`, `revocations_unapplied`) and gate stdio MCP starts (`morph_services::sync::mcp_may_start`). Touch point in Morph Dev's `morph-desktop/src/mcp_host.rs`: one check before `connect` that disables a stdio server `mcp_may_start_in` refuses. See docs/library-sync.md "What a revoked device wrote" | Morph Lead Dev | done (2026-11-18). Not yet: stop an already running server when its writer is revoked (Morph Dev: the host reloads configs only on change); approval fingerprint ignores env values (`morph-ai`, Morph Dev) |
| Long-term research projects (m3talsmith, 2026-11-19; design Page "Morph Companions — Long-term Research Projects", decisions 1 to 9 decided, decision 3 changed: Always allow for the research tools lifts the budget caps). **Progress (restart here):** slice 0 (the spike), slice 1 (Research tab, Make this a research project) and slice 2 (steps run as real companion tasks, dfd5c02) are pushed. Slice 2: `CompanionView::drive_projects` (called from `poll`) asks `research::next_action` for the first running project (caps unless Always allow, finish when no step is left) and starts the step through `Worker::run_with` as a task of the project's companion with `TaskOptions { budget: Some(session_budget(allow)), always_allow, signed_in only with the project's own opt-in }`; `project_finished` calls `research::finish_step` (finding with its source URLs, pages and tool time into `Usage`, step back to the queue and retried once, project paused on a failed step or Done when none is left; a stopped project keeps its state, Stop cancels the task, Pause lets the step end); `ResearchProject::step_prompt` gives the brief, notes, last three findings and this step only; `PanelApprover` answers Navigate and Research cards for a task with Always allow (`Autos`, `auto_answers`; never Interact cards); the detail shows Findings. Dev: `MORPH_COMPANION_AUTOPROJECT=<question>` (+ `MORPH_COMPANION_AUTOPROJECT_ALLOW=1`) starts a project. Real run (Xvfb :118, `<scratch-dir>/scratch/morph-research-e2e/run2.sh`, local Gemma): 3 of 3 steps done, 3 findings, about 12 minutes, `~/morph-shots/research-run.png`. Open: project data does not sync yet; power is assumed on mains; tool time is the task's own `usage.elapsed_ms`; the screenshot sources line only shows when a step had citations. Slice 1 text follows. Slices 0 and 1 are pushed, and the global Site rate setting (m3talsmith, 2026-11-19: `AgentSettings::site_rate`, requests a second to any one site, default 0.5, fractions allowed, in Settings > Agents, synced with the agent settings; each wait is 1/rate plus a random 0 to 10%, never shorter; 429/503 doubling back-off on top; applies to all companions' research fetches and searches through `companion_web::polite_wait`, and to projects through `SiteLimiter`). Slice 0: `morph-ai` `research.rs` (`ResearchProject`, `Projects` in one file with `always_allow_all`, `PlanStep`, checkpoint saved before and after every step, `recover` retries a step in flight once, `run_session` over a `StepRunner` trait, `Caps` and `check_caps` (day 60 pages and 90 min, 300 pages, 14 days, 30 pages an hour), `session_budget`, `SiteLimiter` (the Site rate with jitter, doubling back-off on 429 or 503, always on), `may_run` (power only, pause under 30%, defer when busy)). Always allow (per project or all projects) skips the caps; it never lifts Off sites, signed-in (its own flag), tier 2 cards, Pause and Stop, power rules or the site limiter. Slice 1: `morph-desktop` `companion_research.rs` (the tab's state and drawing, `companions/research.json`; Start project from the composer; Pause/Resume/Stop and Always allow (project, all projects) buttons only set state: slice 2 now runs them), `companion_view.rs` (tabs, Make this a research project chip). Next: steering (4a) and sync. What moves to `weirdnet-agent` 0.4.0 later (needs m3talsmith's OK): the project and checkpoint model, caps, `SiteLimiter`, `may_run` as a `RunPolicy`, the session runner | Morph Dev | slice 0 and 1 done with tests; screenshot `~/morph-shots/research-tab.png` (Xvfb :118, `MORPH_COMPANION_DEMO_RESEARCH=1` seeds three projects; the panel opens with `MORPH_TEST_ACTIONS=shell.toggle_companions` once it exists) |
| Companion browser and research hosts on the page bridge (m3talsmith, 2026-11-18, v0.3). **Progress (restart here):** all slices pushed, including the wiring onto Lead Dev's bridge requests (2026-11-19): research reads in throwaway host pages (`HostPageRequest::new`, `signed_in` only with the per-task opt-in, 15 s timeout, plain HTTP only on `Unsupported`) in b57676c; page-world note only for `PageWorld` in 8cfe900; `page.click`/`page.fill`/`page.scroll` and the card's outline as bridge page actions (`Click`, `Fill`, `Highlight` 30 s, `ScrollBy`, `ScrollTo`) on `active_tab_action`, always `.on_origin` of the address the call was approved for and never page world, typed refusals as clear tool results, in e2bf802; a locked profile (profile lock) refuses every host call with `HostError::Locked`, starts no task and stops running ones in bbf3bce. Linked devices follow the panel's lock check (`CompanionDevices::with_lock`, clippy `-D warnings` clean with and without `--tests`) in d06480f. Nothing of the bridge wiring is left to do. Round 3 (2026-11-19, in order): "+5 pages" card when research ran out of pages (a follow-on task limited to the extension, same conversation, then the normal budget) in 66eadc8; the panel's working line (Thinking…, Loading <model>…, Writing…, the tool's own step line from a `Reporting` tool source such as "Searching DuckDuckGo…"/"Reading tokio.rs…", Waiting for your approval, with elapsed seconds; text already streamed, the silence was model load and prompt prefill) in 03e691e; the panel folds to a 40 px strip (», Ctrl+Shift+K folds and unfolds, × hides; strip pulses while running, ! while a card waits; `companions/panel.json`, not synced; shell.rs only narrows the Companion slot, `morph-ui` layout untouched) in 02299d0. weirdnet-agent (unreleased, m3talsmith asked; no version bump): tool-time-only `per_task` 32d549e and `Approver::extend_pages` 9b405d0. Round 4 (2026-11-19): research that answers from search results without any `web.read` (and no declined card or skip) is re-run once with a short `[Morph, not the user]` nudge (`READ_NUDGE`, `ViewUpdate::Redo` replaces the first answer; not counted as a user turn; logged as "nudged once to read") in 5052176 and cb511f2; `page.click`/`page.fill`/`page.scroll`/`browser.find` say they act on the user's current tab, the system prompt says to call them at once and to read 1 to 3 results before a web answer, and a question with nothing attached starts with `[Current tab: "title" <url>]` (no query or fragment, title cleaned, 100 chars) in f42975d; the card's outline keeps its `HighlightId` and is cleared with `ClearHighlightOne` on allow, decline, expiry, stop or a newer card (`CardOutline`) in 021341a; Ctrl+Shift+K fold logic as a tested helper plus a keymap test in 8f00d25 (an XTest check in a second Xvfb showed the key folds and unfolds with the page, the composer or the address bar focused, so a smoke test where it did nothing likely sent synthetic `XSendEvent` keys, which GPUI and WebKit ignore; Escape in the composer also folds now). `MORPH_COMPANION_AUTOASK` waits for the tab's address so dev runs get the current-tab line (d2ef9b0); the local model's tool calls are read the way Gemma 4 E4B writes them (arguments only, mapped to the one offered tool they fit; `parameters`/`tool` keys; native `call:` syntax inside `<tool_call>`; loose names), where weirdnet-model's `LocalModel` dropped them as nameless calls, in `morph-ai` `local_chat.rs` (`LocalChat`, replaces `LocalModel` under `NativeCalls`) a15086b; card outline lasts `HIGHLIGHT_MAX` 30 s (940c375). Real runs (Xvfb :117; load 1 min avg): before (old prompt, load 4 to 30) research read a page 1 of 4 times, fill/click 0 of 2 called the tool (asked which page); after, at load 33 to 90, research R1, R2, R4 hit the 10 minute wall after the search (inconclusive) and R3 answered from knowledge without searching (no nudge, by design); after a15086b at load 17 to 24, fill Email, fill Name and click Subscribe each called the tool at once (card, Allow once, done; 3 of 3), research R1 and R2 were each nudged once and then read a page (tokio.rs/tokio/topics/shutdown; docs.rs CancellationToken) and answered with footnotes (2 of 2; the nudged task took about 465 s each). Logs in `<scratch-dir>/scratch/morph-hosts-e2e/logs/`. weirdnet-agent 0.3.0 is published (m3talsmith approved, 2026-11-19; release 58cc9c9, tag v0.3.0; https://crates.io/crates/weirdnet-agent/0.3.0, https://docs.rs/weirdnet-agent/0.3.0): `per_task` counts tool time only; adds `ToolTimer`, `time_tool`, `wall_elapsed`, `Approver::extend_pages`, `PagesPrompt`. weirdnet-mcp 0.4.0 (c7e4fc9) and weirdnet-model 0.3.0 (0fbcd3f) are published on agent =0.3.0 (m3talsmith approved, 2026-11-19). Morph is on them in 523d4ac (agent =0.3.0, mcp =0.4.0, model =0.3.0) and 4bbc985: `research_budget()` is `Budget::default()` (90 s of tool time); `PanelApprover::extend_pages` shows the +5 pages card while the task runs (+N pages or No; Stop or the task ending is a no; the follow-on card stays only for tasks that never asked, such as Node tasks); `WallGuard` stops a task after 15 minutes of its own time (card waits not counted) and says why. Checked with --locked: fmt, clippy with and without --tests, clippy for x86_64-pc-windows-gnu, the workspace tests in Xvfb (761 passed); one real research run on agent 0.3 read tokio.rs after the nudge and finished on 2.6 s of tool time. Next: real runs of the mid-task +5 pages card (needs a question that reads 12 pages) and of the wall guard; the 10 minute note is gone. Earlier plan, done: pin it (with weirdnet-mcp and weirdnet-model), `research_budget()` back to `Budget::default()` and its test, `PanelApprover::extend_pages` shows the +5 card mid-task (keep the follow-on for remote tasks, `remote::task` does not forward it), a panel-side wall-clock guard since `per_task` no longer bounds wall time, then real runs. Real runs in Xvfb :117 (2026-11-19, `<scratch-dir>/scratch/morph-hosts-e2e/run.sh`): a footnoted research answer (web.read in a throwaway host page, footnote [1] backed by its fetch step) and a tier 2 fill on a local page (card outline, Allow once, the page's own input listener saw the value), `~/morph-shots/companion-footnotes-real-run.png`, `companion-tier2-fill-*.png`. Files: `morph-ai` `host.rs` (`BrowserHost`, `ResearchHost`, `PageSnapshot`) and `native_calls.rs` (Gemma 4's own tool-call syntax); `morph-companion` `tools.rs` (browser tools tiers 0 to 2, `web.search`/`web.fetch`/`web.read`), `citations.rs`, `session.rs` (`TaskOptions`, `set_budget`); `morph-desktop` `companion_host.rs` (hosts, `PanelApprover`, a task's tools = browser and research beside the companion's MCP servers), `companion_web.rs` (logged-out HTTP research with no cookies, DuckDuckGo or Google), `companion_view.rs` (attach on send, host calls, footnotes, `companions/tool-log.jsonl`, Markdown export), `settings_view/agents.rs` (Add for Always allow). Grants only through `morph_ai::Grants` (the existing synced record); nothing in `morph-core/src/sync.rs` or `morph-services/src/sync.rs`. Uses Lead Dev's bridge without changing it (`read_active_tab`, `read_tab`, `open_host_page_with`, `read_host_page`, `close_host_page`, `active_tab_action`) | Morph Dev | done with tests; screenshots in Xvfb. Open: the mid-task +5 pages card and the 15 minute wall guard are tested but not yet seen in a real run; Google results parsing is best-effort |
| Website: `site/` (multipage: /, /features, /roadmap, /download) and its build check in `.github/workflows/site.yml`. Screenshots of released builds only, in Xvfb with a demo profile | App Maker | live 2026-11-18 for 0.2.0; updated after each release (bump `site/lib/site.dart`, sizes in `pages/download.dart`, move items in `content/features.dart` and `pages/roadmap.dart`) |

### Bridge requests from the companion hosts (for Lead Dev)

Written down, not edited; the hosts work around each one today. **Status (Lead Dev,
2026-11-19): all four landed on `main`, API in docs/bridge-api.md. Wired into the hosts by
Morph Dev the same day (b57676c, 8cfe900, e2bf802).** For Morph Dev to wire
in the hosts: 1. `Browser::open_host_page_with(HostPageRequest::new(url))` (throwaway, the
default; `HostPageRequest::signed_in(url)` for the opt-in), so logged-out research can use
hidden pages with JavaScript instead of `companion_web.rs`'s plain HTTP; 2 and 3.
`Browser::tab_action`/`active_tab_action`/`host_page_action` with `page_action::Action`
(`Click`, `Fill`, `Focus`, `Read`, `Highlight`, `ScrollBy`, `ScrollTo`), pass
`.on_origin(approved_origin)`; errors are typed (`NotFound`, `NotInteractable(why)`,
`BlockedByIsolation`, `WrongOrigin`, ...); 4. `PageData.isolation` is `ScriptWorld` on
macOS and Windows now, so the tools' page-world note can go (keep it for `PageWorld`).

1. **(done)** **Throwaway-session host pages.** `Browser::open_host_page` uses the window profile's data dir, so a hidden page is always signed in. Logged-out research (the default) needs an ephemeral (no cookies, no storage) host page; until then it runs over plain HTTP without JavaScript (`companion_web.rs`), and hidden host pages are used only for the per-task signed-in opt-in.
2. **(done)** **A tab host script for tier 2.** `page.click`, `page.fill` and an element highlight need a script-world call into a tab (find an element by text or selector, outline it, click it, set a field's value with input events). Today the card highlights with the find bar on the target text, and click and fill return "not supported" after approval.
3. **(done)** **Scroll for a webview tab.** `page.scroll` goes through `page_input`, which only works on shell-drawn pages.
4. **(done)** **PageWorld reads.** On macOS and Windows `PageData.isolation` is `PageWorld`; the tools add a note that the page's scripts could have changed the text. A script world there would remove the note.

## Where things stand

- The `gpui` branch was merged into `main` (fast-forward, `fbb5af9`) on 2026-11-14.
  **All work is committed and pushed straight to `origin/main`, no pull requests.**
  Another worker pushes the website (`site/`) to `main`, so always
  `git pull --rebase origin main` before pushing; never force-push. The remote
  `gpui` branch is left as it was.
- Phase 1 (below) is in `main`: crates `morph-ui`, `morph-webview`,
  `morph-browser`, `morph-desktop` (binary `morph`), the Arch recipes, the Xvfb key
  checks and the CI jobs.
- Phase 2 (below) is in `main`: the Flutter desktop targets are gone, so `app/`
  is the mobile app only (Android, iOS); the logic both shells share is in the
  new `morph-services` crate.
- The throwaway spike from planning stays in `docs/evidence/gpui-spike/` (not a
  workspace member).
- Builds are tagged `v0.2.0-alpha.N` from `main` (chosen over `v0.2.0-gpui.N`
  since the work is on `main` now), `pkgver` `0.2.0alphaN`. Only `morph-desktop`
  carries that version.
- CI on `main`: the `flutter` and `android` jobs failed from `fbb5af9` until
  phase 2 (a password-field widget test, and the Android Gradle Plugin 9.1.0
  being below Flutter's 9.1.1 minimum); phase 2 fixed both. The Flutter `linux`
  and `windows` jobs are gone with their targets.

## Phase 7: macOS and Windows builds of the GPUI shell

- **What changed:** `morph-desktop` builds on macOS and Windows; the Linux-only
  parts (`backend`, `render_check`, the raw-key patch) stay behind
  `cfg(target_os = "linux")`. `morph_webview::wry_native` places wry's
  WKWebView (macOS) and WebView2 (Windows) as child views of the GPUI window
  behind the same `Backend`/`WebView` traits; `morph_webview::native_page` is
  the page script those engines need (address changes the page makes itself,
  focus, shell shortcuts while the page has the keyboard, the page bridge,
  find in page over wry's IPC) and is unit-tested on every OS. macOS: system
  title bar, every Ctrl shortcut also takes Cmd, single instance over the Unix
  socket. Windows: GUI subsystem (attaches to the parent console for
  `--version`), single instance over a loopback TCP port named in the socket
  file. Default-browser registration says "not on this system yet".
- **Packaging:** `scripts/macos-dist.sh` (universal `Morph.app`, arm64 +
  x86_64 via lipo, ad-hoc signed, zip + dmg), `scripts/windows-dist.ps1`
  (zip with `morph.exe`, MSI from `packaging/windows/morph.wxs`, WiX 5).
  Unsigned: Gatekeeper and SmartScreen steps in `docs/install-macos-windows.md`.
- **CI:** `desktop-macos` and `desktop-windows` run clippy and the shell's
  tests, build the artifacts and run `scripts/desktop-smoke.py` (a window,
  a page from a local server, the bridge ping/pong, a `pushState` address,
  a screenshot artifact); Windows also installs the MSI, runs
  `morph --version` from Program Files, checks the Start menu shortcut and
  uninstalls. The macOS smoke screenshot shows the tab strip, address bar and
  the page in place.
- **macOS runner (2026-11-19):** with the repo variable `MAC_RUNNER=on`,
  pushes to main, tags and manual runs build
  `desktop-macos` (clippy, tests, `scripts/macos-dist.sh`) on
  m3talsmith's Apple silicon Mac, the repo runner `mac-runner-ios` (label
  `weirdbrains-mac`; it also runs `ios.yml`), with a throwaway `HOME`,
  `MORPH_HOME` and target dir deleted after the job, and nothing installed.
  Pull requests and `release/**` pushes build on `macos-latest`. The smoke
  test is its own job, `desktop-macos-smoke`, always on `macos-latest`
  (windows and screenshots never on m3talsmith's Mac); it downloads the
  `morph-macos-universal` artifact. Moving the runner to the WeirdBrains org
  waits on m3talsmith's `admin:org` sign-in. Xcode 26's Metal Toolchain
  (GPUI's build-time shaders) was installed on the Mac with m3talsmith's OK
  (2026-11-19, `xcodebuild -downloadComponent MetalToolchain`, 17F42) and
  `MAC_RUNNER` is `on`; the first Mac build passed (run 44582305417, about
  18 min, peak about 6 GB of disk, all freed by the cleanup step). On the
  Mac a newer push replaces an older `desktop-macos` still waiting (job
  concurrency), so pushes don't pile up. `ios.yml` deletes its build output
  (`app/build`, `app/.dart_tool/hooks_runner`) at the end of each run.
  Set `MAC_RUNNER` to `off` to send every macOS build back to hosted.
- **Not verified:** real use by a person on either OS (typing into pages,
  IME, Retina scaling, multi-monitor, dark mode), popups (`window.open`
  opens the engine's own window, not counted on the tab), crash/hang panels
  (WebView2 reports no crash; no hang detection on either), links from other
  apps on macOS (Apple Events not wired), default-browser registration,
  per-profile storage on macOS older than 14, Windows on ARM, signing and
  notarization.

## alpha.7: alpha.6 on a real GPU, and the render check

alpha.6 on my-desktop (Hyprland, NVIDIA GPU, nvidia-open 615.78.08, EGL vendor
NVIDIA, webkit2gtk-4.1 2.54.1, session `GDK_BACKEND=wayland GDK_SCALE=2`):

- **Native Wayland aborted** at start: `GDK is not able to create a GL
  context: The current backend does not support OpenGL` (also with
  `MORPH_DMABUF_PROBE=0`). Cause, reproduced on the box: GTK 3 off-screen
  windows never make GL contexts, and WebKit `g_error`s when it gets a GPU
  buffer for such a view. The box has no DRM device, so WebKit used shared
  memory there and never hit it.
- **X11 (`MORPH_BACKEND=x11`) pages black**, `Failed to create GBM buffer of
  size 1x1: Invalid argument` twice. The alpha.6 probe (2x2 ARGB8888 with the
  GBM-platform EGL display's modifiers) did not ask for what WebKit asks for
  (opaque XRGB8888, modifiers intersected across the UI and web process
  displays), so it passed.
- **Fix (`crates/morph-desktop/src/render_check.rs`,
  `crates/morph-webview/src/render_probe.rs`):** child processes
  (`morph --probe-render offscreen|window gl|page`) ask GTK whether an
  off-screen window has GL (Wayland: if not, `WEBKIT_DMABUF_RENDERER_FORCE_SHM=1`,
  always checked where a DRM device exists) and load a test page with a known
  colour where the shell's pages live, reading the view back until the colour
  shows; the parent also stops a try when WebKit prints a buffer failure.
  Ladder: defaults, `WEBKIT_DMABUF_RENDERER_DISABLE_GBM=1`,
  `WEBKIT_DMABUF_RENDERER_FORCE_SHM=1`, `WEBKIT_DISABLE_DMABUF_RENDERER=1`;
  Wayland falls back to XWayland when nothing paints natively. One line on
  stderr (`morph: ...`) says what was chosen. `MORPH_DMABUF_PROBE=0` skips the
  page check (not the GL one), `=1` forces it without a DRM device. Any
  WebKit renderer variable the user sets skips the page check. No vendor is
  named anywhere.
- **Checked:** unit tests with made-up outcomes (m3talsmith's two failures, a
  working GPU, XWayland fallback, no conclusion); `xvfb-check.py` runs the
  child under Xvfb and sway (off-screen window has no GL; test page paints;
  a Morph with the check forced starts, on Wayland with shared memory).
  **Not verified on NVIDIA** (no GPU here): m3talsmith's run is the test.
- The `_gdk_frame_clock_freeze` critical on X11 predates alpha.6 (alpha.5
  prints it on the box) and does not affect drawing.
- The GDK_SCALE harness check compared against the first page report, which
  can come before GTK knows the output's scale (one scale-2 run failed that
  way, the rerun passed); it now waits for both to settle.

## Phase 6 result: what changed and how it was checked

Native Wayland (`v0.2.0-alpha.6`).

- **Backend choice** (`crates/morph-desktop/src/backend.rs`): on a Wayland
  session GPUI runs natively and pages come from `morph_webview::offscreen`;
  on X11 nothing changed (child windows). `MORPH_BACKEND=x11|wayland`
  overrides; if GTK cannot open Wayland the shell falls back to XWayland.
- **Off-screen pages** (`crates/morph-webview/src/offscreen.rs`): each page is
  a WebKitGTK view in a `gtk::OffscreenWindow`, embedded in one realized,
  never-shown `gtk::Window` so it gets the monitor's scale (devicePixelRatio
  matches the shell). Damage marks the frame dirty; the shell copies it (BGRA,
  device pixels) into a GPUI image only then. Overlay scrollbars are off on
  Wayland (`GTK_OVERLAY_SCROLLING=0`): their fade animation redrew idle pages
  about 12 times a second. Pages are built inside the embedder window and
  then moved, hidden from WebKit, into their offscreen window, because WebKit
  loads media only for pages whose top-level widget is an on-screen
  `GtkWindow` (every `<video>` stayed empty before).
- **Input:** pointer, wheel and keys become synthetic GDK events; keys carry the
  real XKB keycode, keysym and state from a vendored `gpui_linux`
  (`third_party/gpui_linux`, `raw_key::current()`). Input-method commits come
  through a GPUI `InputHandler` (no inline preedit yet).
- **m3talsmith's bugs, fixed in alpha.6:** pages twice the shell's size with
  `GDK_SCALE=2` (GDK_SCALE/GDK_DPI_SCALE are removed before GTK starts and
  pages zoom by the shell's scale); the LinkedIn sign-in popup staying open
  after `window.close()` (wry's `close` handler destroys the web view and
  drops ours; the window now follows the web view's `destroy`); videos not
  playing on Wayland (above). On X11 the bundled clips already played.
- **No vendor checks (m3talsmith, 2026-11-15).** The NVIDIA detection pushed in
  `b706629` is removed (`3261b1c`). (alpha.6 state; alpha.7 replaced this
  probe with the render check above.) Instead `morph --probe-dmabuf` ran as a
  child at start and makes the allocation WebKit's DMA-BUF renderer makes (2x2
  ARGB8888 GBM buffer for rendering, with the EGL display's modifiers, on the
  first render node). Only if it fails does Morph set
  `WEBKIT_DMABUF_RENDERER_DISABLE_GBM=1` (GL-texture export, still GPU) or,
  without `EGL_MESA_image_dma_buf_export`, `WEBKIT_DMABUF_RENDERER_FORCE_SHM=1`.
  Any user-set WebKit renderer variable, or `MORPH_DMABUF_PROBE=0`, skips it.
  GL compositing stays on; the hardware-acceleration policy is never set
  (`Never` gave blank off-screen pages). **Not verified on NVIDIA** (the box
  has no GPU: the probe says "unavailable" and changes nothing). WebKit has no
  knob to pick modifiers (`WEBKIT_DMABUF_RENDERER_BUFFER_FORMAT` picks the
  format), so true modifier negotiation would need a WebKit change.
- **VA-API:** `GST_PLUGIN_FEATURE_RANK` ranks the `va` plugin's decoders
  (H.264, H.265, VP8, VP9, AV1, MPEG-2) first unless the user set the variable
  or `MORPH_VAAPI=0`. The `va` plugin (gst-plugins-bad) registers only what the
  VA driver can decode, so it is a no-op without VA-API. Not verified on real
  hardware here.
- **Checks:** `scripts/xvfb-check.py --wayland [--scale 2]` runs the same
  suite on a headless sway (`WLR_BACKENDS=headless`, pixman renderer),
  driven by `morph-wl-input` (virtual keyboard with the standard evdev/us
  keymap, virtual pointer, input-method commits); CI runs Xvfb, Wayland and
  Wayland scale 2. New checks on both: popup posts to its opener and calls
  `window.close()` (window gone, PopupOpened/PopupClosed balanced), bundled
  VP8 WebM and H.264 MP4 play (`currentTime` advances), `GDK_SCALE=2` does not
  change the page size. Last local run: Xvfb 160/160, Wayland 155/155, Wayland
  scale 2 155/155. No key behaves differently between Wayland and X11; the
  Wayland run lacks only the X11-only frameless-window checks (xprop/xfwm4).
- **m3talsmith tests:** alpha.6 on Hyprland natively (no XWayland): page
  sharpness at his scale, typing (IME too), LinkedIn sign-in popup closing,
  YouTube/video playback, whether pages are black (if so, the probe's log
  line and whether `MORPH_DMABUF_PROBE=0` or `WEBKIT_DISABLE_DMABUF_RENDERER=1`
  changes it), CPU while idle, and `MORPH_BACKEND=x11` for comparison.

## Phase 5 result: what changed and how it was checked

Packaging, the page bridge and the Wayland question (`v0.2.0-alpha.5`).

- **Page bridge round trip** (carried over from phase 4): every page and popup
  gets `morph-bridge.js` (the same file the mobile app injects,
  `morph_core::page_bridge::BRIDGE_SCRIPT`) and a shim posting through a WebKit
  script message handler of Morph's own (`morphBridge`; wry's IPC drops
  messages from `file://` pages because their URL is not an `http::Uri`).
  Replies, errors included, come back through `MorphBridge.deliver`
  (`morph_webview::PageBridge`, `Backend::set_page_bridge`).
- **The flaky "Left moves the tabs to the left (saved)" check:** the cause
  was `xdg-settings` (the default-browser status on opening settings, and
  "make default") running on the UI thread; on a slow desktop the window
  stopped answering and the arrow key after `d` was saved seconds late. It now
  runs on a thread (`default_browser::Check`) and the settings panel picks the
  result up on the host's tick. The Xvfb checks give Morph an `xdg-settings`
  that takes 2 s and check that the keys did not wait for it (a regression
  test for the cause). Green since: 2 local runs (151/151)
  and CI on `2a076b1`..`854bf68` (3 runs).
- **Linux packages** (`scripts/linux-packages.sh`, from the release tarball):
  a zip, `morph` and `morph-standalone` debs and rpms (with and without the
  desktop entry and icons; `0.2.0~alpha.5` sorts before 0.2.0), and a Nix flake
  (`flake.nix`, `packaging/nix/`) with `morph` and `morph-standalone` built
  from the release named in `packaging/nix/release.json`. Checked: the debs
  installed in `ubuntu:24.04`, the rpms in `fedora:latest` (`morph
  --version`, desktop file and icons only in `morph`, `dnf swap` to the
  standalone), the flake built in `nixos/nix` (`nix flake check`, both
  packages, `morph --version`); CI builds and installs the deb, checks the rpm
  and builds both flake packages (`nix` job). Not made: a standalone Arch
  recipe, AppImage and Flatpak (not in the plan), aarch64.
- **Release script:** `scripts/weirdnet_release.py` (helpers 0.3.0, vendored
  byte-identical) and `release.toml` (platform matrix, `morph-bin` TAUR
  recipe, `[github]` assets). Since helpers 0.3.0 it cuts the phase
  pre-releases: `--pre alpha` bumps `0.2.0-alpha.N` to `alpha.N+1`, `{pkgver}`
  is the Arch form (`0.2.0alpha8`), `taur-check` expects it, and `github
  VERSION` creates the pre-release with a SHA256SUMS listing GitHub's dotted
  asset names. (Before 0.3.0 the pre-releases were cut by hand.)
- **Wayland:** native Wayland is **not done**. Checked instead under headless
  sway (`WLR_BACKENDS=headless`, in an `archlinux` container): with XWayland
  Morph runs as an XWayland client (the page loads, a key reaches it); without
  XWayland its window opens natively and shows the explanation instead of a
  page. A probe showed the way forward: an offscreen `WebKitWebView` under
  GDK's Wayland backend renders (`gtk_offscreen_window_get_pixbuf`) and takes
  synthesized clicks and keys, so a native path can draw the page as a GPUI
  image and forward input (the snapshot fallback planned in ARCHITECTURE.md).
  That is a backend of its own plus page drawing and input in the shell.
- **Checks on this box:** fmt, build, clippy `-D warnings`, workspace tests
  clean; Xvfb checks 151/151 on the release build (phase 4's plus the bridge
  and the slow `xdg-settings`).

## Phase 4 result: what changed and how it was checked

Identity and links in the GPUI shell (`v0.2.0-alpha.4`).

- **First key after closing a panel (fixed first).** WebKit hands a key the
  page did not use back through GTK and expects to see that copy again; the
  shell's `HostKeys` stopped the copy, so WebKit skipped the next real key.
  `SeenKeys` (`morph-webview`) lets those copies through. Proof:
  `scripts/xvfb-focus-loop.py --runs 50` for Ctrl+, / Escape, Ctrl+H / Escape
  and Ctrl+F / Escape, 50 of 50 keys reached the page in each (150 total; about
  1 in 4 was lost before), and the Xvfb checks press every key exactly once.
- **Identity panel** (`morph.identity`, Ctrl+Shift+L, `identity.rs` model and
  `identity_view.rs`): without an identity, 1 create (password twice; the 24
  recovery words are shown once), 2 restore from the words, 3 restore from a
  backup file (its password, a new profile password), 4 link from another
  device. With one: w show the recovery words (password), b save a backup
  (profile password, a backup password, a path; the file is written 0600 and
  never overwrites one), a add a device. Every way in encrypts the profile's
  library (`morph-services`), and the shell opens it right away.
- **Device linking** (`morph_services::device_link`): the new device shows a
  link and its QR code (`qrcode`, drawn in GPUI) and a Copy link button; the
  other device pastes the link with its password; both show the same short
  code, Enter on each confirms, and the new device chooses its own password.
  Sessions end when the panel closes.
- **Default browser** (settings panel, `d` or the button): `xdg-settings set
  default-web-browser morph.desktop`, then reads it back; explains a set
  `$BROWSER` and a missing `morph.desktop`. `morph.desktop` already lists the
  http/https/html MimeTypes.
- **Single instance** (`single.rs`): one Unix socket per Morph home in
  `$XDG_RUNTIME_DIR` (`morph-<hash>.sock`); a second `morph URL` hands the URL
  over (a new tab in the running window, which is raised) and exits; a second
  `morph` alone raises the window. A stale socket is replaced.
- **Library:** after unlocking, the keyboard goes to the search (it was left
  nowhere, so Escape did nothing).
- **Checks on this box:** fmt, build, clippy `-D warnings`, `cargo test
  --workspace` clean; Xvfb checks 146/146 on the release build: everything
  from phase 3 plus the default browser (in an isolated XDG environment),
  single instance, creating an identity, saving a backup, restoring it in a
  second home, linking a third home (offer, add, confirm on both, password)
  and, after a relaunch, unlocking the library with the identity's password.
- **Not done:** the page bridge round trip listed for phase 4.
- **Not checked:** linking with the phone app (only desktop to desktop under
  Xvfb), the real system keyring (Xvfb has none, so the library asks for the
  password at each launch there), `xdg-settings` on m3talsmith's desktop and
  links opened from other apps, the QR code with a real camera.

## Phase 3 result: what changed and how it was checked

Everyday browser parts in the GPUI shell. Panels open in the sidebar slot beside
the page (the page narrows, never covered); each is a component behind the
`Hosted` trait in `shell.rs` and asks the host for things with `ShellRequest`s
(open a URL, close the panel, take the keyboard, switch profile, settings
changed), so the browser and the panels do not know each other.

- **Find in page** (Ctrl+F; F3/Ctrl+G next, Shift+F3/Ctrl+Shift+G previous): a
  find bar row between the toolbar and the page, with the match count; Escape
  closes it and gives the page the keyboard back. WebKit's `FindController`.
- **Error pages:** a load that fails (unreachable, DNS, TLS, refused, ...) shows
  Morph's page instead of the web view, with the reason and "Try again"; the
  address bar keeps the failed URL; F5 retries it, Alt+Left shows the page
  under it.
- **Crash and hang panel per tab:** a crashed web process shows "This page
  stopped working" with Reload; an unresponsive one shows Wait and Reload
  (Reload ends the hung process).
- **Library** (`morph.library`): Ctrl+D bookmarks the page or removes the
  bookmark (the ☆/★ in the toolbar does the same); Ctrl+Shift+O opens
  bookmarks (folders, tags, search, move to folder), Ctrl+H history (search,
  "5 min ago"). Enter opens the first match; the same key or Escape closes it.
  If the profile's library is locked (no key in the keystore) the panel opens
  at start with a password prompt; unlocking turns history on.
- **Profiles** (`morph.profiles`, Ctrl+Shift+M or the profile pill in the
  toolbar): switch, create (and switch to it), rename, delete (not the shown
  one; its saved key goes too). Switching rebuilds the window's browser and
  library for that profile (its tabs, cookies, layout, settings).
- **Settings** (`morph.settings`, Ctrl+,): tab position top, bottom, left or
  right, and for left/right a collapsed list (one letter per tab, 48 px) and its
  width (160–480 px). Saved per profile in `settings.json`; arrow keys move the
  tabs while the panel has the keyboard, `c` collapses, `-`/`=` resize.
- **Frameless window:** the window asks for client-side decorations. Where GPUI
  can draw them (the window manager supports `_GTK_FRAME_EXTENTS` and a
  compositor runs: KWin, Mutter, xfwm4, ...), there is no title bar: the tab row
  (or the toolbar when the tabs are elsewhere) is the caption; drag it to move
  the window, double-click to maximize, minimize/maximize/close on the right,
  a 4 px border to resize by. On tiling window managers (Hyprland, Sway, i3,
  niri, ... from the session variables, `morph_ui::chrome::window_buttons`)
  only close is drawn, also when the WM keeps its own (no) frame;
  `MORPH_WINDOW_BUTTONS=close|all` overrides. Elsewhere (no compositor, e.g.
  openbox alone) the WM's frame stays.
- **Skipped:** downloads to a chosen folder (optional for this phase).
- **Checks on this box:** `cargo fmt --check`, `cargo build --locked`, `cargo
  clippy --workspace --all-targets -D warnings`, `cargo test --workspace` all
  clean; Xvfb checks 119/119 on the `0.2.0-alpha.3` release build (find, error page,
  library, profiles, settings including a relaunch with saved settings, and the
  frameless window under xfwm4 with its compositor: no frame, drag, maximize,
  close). In an `archlinux` container: `morph-bin` built (`makepkg`), installed (`pacman -U`), `morph --version` printed `0.2.0-alpha.3` and its window opened under Xvfb; for the source recipe `morph` only `makepkg --printsrcinfo` (the CI `arch-package` job builds it).
- **Fixed after the release (phase 4 start): the first key after closing a
  panel.** WebKit hands a key the page did not use back through GTK, starting
  at the page's window, and expects to see that copy again (it skips the next
  key it gets otherwise). While the shell had the keyboard, `HostKeys` sent
  that copy (a late Ctrl press from Ctrl+, or Ctrl+H) to the shell and
  stopped it, so the page dropped the next real key. `HostKeys` now remembers
  which key events the web view took in and lets their copies through
  (`SeenKeys` in `morph-webview`). `scripts/xvfb-focus-loop.py` checked it:
  50 of 50 keys after Ctrl+, / Escape, after Ctrl+H / Escape and after
  Ctrl+F / Escape (it was about 1 in 4 lost before); the Xvfb checks press
  each key once again.
- **Not checked:** m3talsmith's Hyprland session (whether XWayland there reports a
  compositor; either way only close is drawn), KDE/GNOME by hand, resizing by
  the border with a real pointer, HiDPI, the library password prompt by hand with a
  real keystore (its model is unit-tested), real sites' TLS errors.

## Phase 2 result: what changed and how it was checked

- **Flutter desktop removed.** `app/linux`, `app/macos`, `app/windows`,
  `packages/morph_webview_linux`, `packages/morph_webview_inapp/macos`,
  `scripts/build-windows.ps1`, the Flutter `linux` and `windows` CI jobs, and the
  desktop-only Dart: window chrome and `window_manager`, the desktop nav row,
  profile button and menu, tab strip, sidebar and tab-position settings, tab
  overflow, the Linux and Windows default-browser code. The shell always builds
  the phone layout (address sheet, tab grid, edge gestures). Kept on purpose:
  keyboard shortcuts and the find bar (hardware keyboards on phones and tablets)
  and the mouse drag path in the bookmark manager (tablets with a mouse).
- **`morph-services`** (new crate, no UI toolkit): Morph home resolution,
  library open/unlock (keystore first, password otherwise, a wrong saved key is
  forgotten), adopting a new identity's key, profile deletion with its saved key,
  identity onboarding (create, import words, restore a backup, recovery words,
  export a backup) and device linking (new device and add device, the link
  registry). 12 tests, including a real TCP link and the mock keystore.
  `morph-bridge`'s `api/*` now call it; every bridge function keeps its signature,
  so the existing Dart bindings only differ in generated comments.
- **Mobile on the Rust address-bar rule.** New sync bridge function
  `omnibox_target` (`morph_core::omnibox::target`); `MorphShell` calls it through
  `MorphApi.omniboxTarget`, and the Dart copy of the rule now only backs
  `FakeMorphApi` in widget tests (held to the Rust test cases).
- **Desktop history with an encrypted library.** At launch `morph` opens the
  current profile's library with the key in the system keystore
  (`morph-services`); if that fails (no key, no keystore, wrong key) history is
  off for the session and a line goes to stderr. The password prompt is phase 3.
- **Mobile fixes.** The password field's show/hide is its own state again (the
  pre-existing `password_field_test` failure); AGP is 9.1.1. Adding a profile
  from the address sheet closes the sheet (the new profile's onboarding used to
  open under it), and the sheet's profile list follows renames and deletes.
- **Checks on this box:** `cargo fmt --check`, `cargo build --locked`, `cargo
  clippy --workspace --all-targets -D warnings`, `cargo test --workspace` all
  clean; `flutter analyze` clean and `flutter test` 115/115 (desktop-only widget
  tests went with their code; the rest run as phone tests), `morph_webview`
  48/48, `morph_webview_inapp` 29/29; Xvfb key checks 72/72 on the
  `0.2.0-alpha.2` release build. No Android SDK here, so the APK build and the
  Gradle unit tests are checked by the `android` CI job only.

## Phase 1 result: what works and how it was checked

On this box (Debian 13, Xvfb, `WAYLAND_DISPLAY` unset, openbox, webkit2gtk 2.54,
Mesa lavapipe Vulkan, Rust 1.100), release build of `v0.2.0-alpha.1`:

- **Window and browser component.** A 1200x800 GPUI window (`app_id` `morph`)
  draws a tab strip, a toolbar (back, forward, reload/stop, address bar) and the
  page below as a webkit2gtk child window. Tabs open, close and switch (buttons
  and keys); the address bar edits text with the mouse and keys, phrases become
  searches (main's rule and test cases, `morph_core::omnibox`); back, forward,
  reload and stop follow the page; history goes into the profile's library; a
  crashed page shows a reload panel in its tab.
- **Popups** (`window.open`) are GTK windows Morph makes, related to the opener
  (same session, `postMessage` works); their title follows the page, F5 and
  Ctrl+R reload them, Ctrl+W closes them, and every other key goes to their page.
- **Keys.** `scripts/xvfb-check.py` starts the binary in its own Xvfb with
  openbox and a temporary `MORPH_HOME`, loads `tests/xvfb/keys.html` (a page that
  puts every keydown, the focused field, the field values, the caret and the
  scroll position in its title), and drives it with `xdotool`. 72 of 72 pass:

| Where | Check | Result |
|---|---|---|
| shell | window opens | pass |
| shell | page loads | pass |
| page | click focuses the input | pass |
| page | typing | pass |
| page | typing with the pointer over the shell | pass |
| page | Backspace | pass |
| page | Home in a field | pass |
| page | Delete | pass |
| page | End in a field | pass |
| page | Left arrow | pass |
| page | Right arrow | pass |
| page | Ctrl+A, Tab moves focus | pass |
| page | Ctrl+C / Ctrl+V | pass |
| page | Shift+Tab moves focus back | pass |
| page | Ctrl+X | pass |
| page | Ctrl+Z | pass |
| page | Enter reaches the page | pass |
| page | Escape reaches the page | pass |
| page | Ctrl+F reaches the page (no find bar yet) | pass |
| page | keypad key, Num Lock off | pass |
| page | keypad digits and +, Num Lock on | pass |
| page | keypad Enter | pass |
| page | Page Down scrolls | pass |
| page | Page Up scrolls back | pass |
| page | Space scrolls down | pass |
| page | Shift+Space scrolls up | pass |
| page | Down arrow scrolls | pass |
| page | Up arrow scrolls | pass |
| page | End scrolls to the bottom | pass |
| page | Home scrolls to the top | pass |
| page | F5 reloads | pass |
| page | Ctrl+R reloads | pass |
| shell | Ctrl+T opens a tab from the page | pass |
| shell | the new tab's address bar takes typing | pass |
| shell | Ctrl+Tab switches tabs | pass |
| page | page never saw Ctrl+T / Ctrl+Tab | pass |
| shell | Ctrl+L, then a phrase searches | pass |
| shell | Ctrl+W closes the tab (tab 2 shows again) | pass |
| shell | F11 toggles full screen | pass |
| shell | F11 again leaves full screen | pass |
| popup | window.open makes a popup window | pass |
| popup | click focuses the input | pass |
| popup | typing | pass |
| popup | Backspace | pass |
| popup | Home in a field | pass |
| popup | Delete | pass |
| popup | End in a field | pass |
| popup | Left arrow | pass |
| popup | Right arrow | pass |
| popup | Ctrl+A, Tab moves focus | pass |
| popup | Ctrl+C / Ctrl+V | pass |
| popup | Shift+Tab moves focus back | pass |
| popup | Ctrl+X | pass |
| popup | Ctrl+Z | pass |
| popup | Enter reaches the page | pass |
| popup | Escape reaches the page | pass |
| popup | Ctrl+F reaches the page (no find bar yet) | pass |
| popup | keypad key, Num Lock off | pass |
| popup | keypad digits and +, Num Lock on | pass |
| popup | keypad Enter | pass |
| popup | Page Down scrolls | pass |
| popup | Page Up scrolls back | pass |
| popup | Space scrolls down | pass |
| popup | Shift+Space scrolls up | pass |
| popup | Down arrow scrolls | pass |
| popup | Up arrow scrolls | pass |
| popup | End scrolls to the bottom | pass |
| popup | Home scrolls to the top | pass |
| popup | F5 reloads | pass |
| popup | Ctrl+R reloads | pass |
| popup | popup state reached the shell | pass |
| popup | Ctrl+W closes the popup | pass |

  The page rows run in the main window, the popup rows in a popup opened from the
  page. "page never saw Ctrl+T / Ctrl+Tab" reads the page's key log: shell keys
  are taken before the page. Ctrl+F goes to the page until find in page exists
  (phase 3). CI runs the same script on Ubuntu 24.04 (`desktop-linux` job) and
  attaches the report as `xvfb-checks.md`.
- **Arch package.** In an `archlinux:latest` container (podman): `makepkg
  --printsrcinfo` matches both `.SRCINFO` files; `makepkg -s` for
  `packaging/arch/morph` built it from the checkout (`cargo build --locked
  --release`), `pacman -U` installed it, `morph --version` printed `morph
  0.2.0-alpha.1`, `pacman -Ql` listed the binary, desktop file, icons, licence and
  readme, and the installed `morph` opened its window under Xvfb. `morph-bin`
  was checked against the published release: the download passed its sha256,
  `makepkg` and `pacman -U` worked, `morph --version` printed `0.2.0-alpha.1`, and
  the window opened under Xvfb.
- **Quality bar** before each commit: `cargo fmt --check`, `cargo build`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
  New tests: `morph-ui` (layout, keymap, text field), `morph-core::omnibox`,
  `morph-browser` (17, against the fake web view), `morph-desktop` (key routing,
  action mapping, UTF-16 offsets).

**Not checked:** a real Wayland session through XWayland (Hyprland on
my-desktop), NVIDIA, HiDPI scaling, IME input, window resizing by hand, real
sites and real sign-in popups, a long session's CPU cost of pumping GTK every
8 ms, and m3talsmith's Arch box itself (packages were checked in a container only).

### How the keys are routed (Wayland)

- The page has no window of its own on screen; the shell owns the keyboard.
  GPUI's keybindings run first, so shell shortcuts never reach the page. Any
  other key, while the page has focus, becomes a GDK key event for the page's
  offscreen window with the keyboard's own keycode, keysym, modifiers and XKB
  group (`raw_key::current()` from the vendored `gpui_linux`), so
  `KeyboardEvent.key`/`code` match X11, keypad and dead keys included. Text
  from an input method arrives as committed text. Focus in/out is sent as GDK
  focus-change events when the page gains or loses focus.

### How the keys are routed (X11)

- The page is a child X window of the shell's window. X11 gives a key to the
  window under the pointer when the focused window is its ancestor, so:
  - **Shell has focus** (address bar, after Ctrl+T or Ctrl+L, a click on the tab
    strip or toolbar): the shell's window has X focus, and the page's GTK window
    hands any key it gets on to the shell's window (`XSendEvent`, `HostKeys` in
    `morph-webview`).
  - **Page has focus** (a click into the page, Escape in the address bar, a
    submitted address): the web view's own X window gets X focus, so keys reach
    the page wherever the pointer is.
- While a page has focus, its web view reports the keys the shell uses (Ctrl+T,
  Ctrl+W, Ctrl+L, Ctrl+R, F5, Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+1..9, Alt+Left/Right,
  F11, Ctrl+Q) and does not pass them to the page; everything else goes to the page.
- WebKitGTK does not bind Ctrl+Z / Ctrl+Shift+Z itself, so a small injected
  script runs undo/redo when the page did not handle the key.

## How m3talsmith builds and installs on my-desktop

Arch, Rust 1.99 (the workspace's `rust-version` is 1.99, so that is the minimum).

```sh
cd ~/src/WeirdBrains/morph
git switch main && git pull --ff-only origin main
sudo pacman -S --needed base-devel webkit2gtk-4.1 gtk3 libxkbcommon-x11 libxcb \
  vulkan-icd-loader fontconfig freetype2 openssl clang cmake pkgconf

# 1. Run from the checkout without installing (a throwaway profile):
export CARGO_TARGET_DIR=<scratch-dir>/cargo-target/morph
MORPH_HOME=$(mktemp -d) cargo run -p morph-desktop --release

# 2. Or build and install the Arch package from the checkout:
cd packaging/arch/morph && makepkg -si      # morph 0.2.0alpha7-1
morph                                        # or "Morph" in the launcher

# 3. Or the prebuilt release from TAUR (conflicts with morph):
yay --aururl https://pkg.taur.weirdbrains.com -S morph-bin
```

`makepkg` builds into `packaging/arch/morph/src/target` (its own target dir, not
`<scratch-dir>`), so the first build takes a while. Without `MORPH_HOME` the
installed `morph` uses the normal profile directory. On Hyprland it runs
natively on Wayland since alpha.6 (`MORPH_BACKEND=x11` for XWayland).

## After alpha.8: the old Flutter desktop entry

The Flutter desktop build left `~/.local/share/applications/org.weirdbrains.morph.desktop`
(`X-Morph-Generated=true`, `Icon=org.weirdbrains.morph`, `Exec` at a removed
temp build) and `mimeapps.list` lines giving it `text/html` and
`x-scheme-handler/http(s)`; launchers showed a broken Morph without an icon.
`crates/morph-desktop/src/legacy_desktop.rs` runs at start-up on Linux, off
the UI thread: it removes that entry only when it carries the marker, drops
the id from `$XDG_CONFIG_HOME/mimeapps.list`, and where it was the default
puts the packaged `morph.desktop` (the id "Make default browser" uses) in its
place. Rides the next alpha.

## alpha.10: desktop chrome

Built on `wip/alpha10`, a commit per feature, then merged to `main`.
- Preferences page (`settings_view.rs`, `Cover::Settings`), search engines
  (`morph_core::search`, per-tab pick in `morph-browser`), the address
  dropdown (`AddressItem`, mirrors the mobile sheet), tab bar buttons
  (position menu, collapse), favicons, page file pickers (`take_file_request`
  answered once), last tab quits (`ShellRequest::Quit`).
- `weirdnet-morph://`: `morph_core::scheme` parses links (log-safe
  `describe`); no routes yet, so a link shows a toast. `crates/morph-os` holds
  the macOS (NSWorkspace / LaunchServices) and Windows (HKCU registration,
  UserChoice check, Default Apps page) default-browser code.
- The MSI registers per machine (HKLM StartMenuInternet, Capabilities,
  RegisteredApplications, `weirdnet-morph`), since the MSI installs per
  machine; a zip install writes the same keys under HKCU at first run.
- Frameless macOS/Windows: `TitlebarOptions { appears_transparent }`;
  `chrome::caption` picks the buttons (`WindowButtons::Native` on macOS leaves
  78 px for the traffic lights). Windows moves the window through
  `WindowControlArea::Drag` areas (the empty tab-strip space, a grip at the
  toolbar's start when tabs are elsewhere); there is no `start_window_move`
  on Windows in GPUI.
- Two-way device linking: `weirdnet_device_link::invite` (an existing
  device's `weirdnet-morph://invite/…`: peer, addresses, a 16-byte token,
  120 s expiry; served on `/weirdnet/link-invite/2`). The new device answers
  with its usual signed offer link; the existing device then pairs with it
  exactly as if scanned, so the SAS and the sealed transfer are unchanged.
  An invite holds no secret and takes one answer. `MORPH_TEST_IDENTITY=offer`
  opens the offer screen for the smoke checks, which decode the QR
  (zxing-cpp) from a screenshot; `MORPH_REQUIRE_QR=1` in CI makes a missing
  decoder a failure.
- Shortcut audit in `morph_ui::actions` (`MAC_RENAMES`, `PC_ONLY`,
  `MAC_ONLY`); the page script reports whatever the keymap binds.
- Xvfb: `bar_keys`, `tab_bar`, `favicon`, `file_picker`, `select_menu`,
  `last_tab_quits`, the star and toast (`morph-event drawn …`).
- Wayland paste crash (alpha.9, SIGSEGV in WebKitGTK's clipboard code on the
  off-screen window): `morph_webview::clipboard` keeps WebKit away from the
  GDK clipboard; the shell reads GPUI's clipboard and dispatches a real
  `paste` event (text, or an image as a file), and writes what a page copies
  or cuts. Middle click: paste the primary selection on editable text, open
  a link in a new tab. Xvfb/Wayland `page_clipboard`.
- ☰ menu: entries emit `ShellRequest::Run(action)` before the menu closes
  (they did nothing before). The Identity toolbar button is gone. Xvfb and
  Wayland `main_menu` click every entry; `desktop-smoke.py` runs every entry
  through `MORPH_TEST_MENU` on all three systems.
- Downloads: backends report `Event::Download` (start with the suggested
  name and the staging file, progress on Linux, end). WebKitGTK 2.54 picks
  the file in the Downloads folder itself before `decide-destination`, so
  Linux uses WebKit's file as the staging file (`created-destination`).
  `morph_browser::downloads` moves the file once the bytes and the save
  dialog's answer (`dialogs::Ask::Download`, the portal on Wayland) are in;
  cancelling stops it. A bar under the page shows progress and the result.
  Xvfb/Wayland `download`: a redirect to an attachment, like a GitHub asset.
- X11: pages no longer get bare Ctrl/Alt/Super presses. WebKit answers a key
  press asynchronously and re-sends unhandled ones through GTK; after Ctrl in
  a page and L caught as a shortcut, that answer sometimes never came and the
  page swallowed its next key (the order-dependent tabs/settings failures).

Mobile parity (desktop features alpha.10 adds that the Flutter app should
match): the address sheet's full list (back, forward, copy link, refresh,
bookmark, find, history, bookmarks, identity, settings, profiles); search
engine picker per tab with a link to the default in settings; settings as a
searchable page; morph://about; favicons in tabs; downloads with a save
location, progress and result; paste with images (clipboardData files);
device linking in both directions: the new device's offer screen (QR, link,
Copy link, SAS compare) and the existing device's "Add a device" screen
(invite QR, link, Copy link, Paste link, Paste from clipboard).

## alpha.9: identity

Scope cut by m3talsmith to identity (everything else moved to alpha.10; the
unfinished alpha.10 work is parked on branch `wip/alpha10`, not built).
- Recovery words: `c` Copy (the clipboard is cleared after 60 s if it still
  holds them; `MORPH_TEST_CLIPBOARD_SECS` for the checks), `d` Download (native
  Save dialog, `morph-recovery-words-<profile>.txt`, mode 0600, note to keep
  it offline).
- Paper shards: `s` in the overview, behind the profile password: 3 of 5,
  weirdnet-keys text form, unsealed (one shard tells nothing). Each has Copy,
  Download and QR; `a` "Download all shards" writes one file per shard into a
  chosen folder and never replaces files. Restore takes shards as files
  ("Choose files…", several) or pasted into the words field.
- Native dialogs for the panel's file fields (`dialogs.rs`, GPUI's prompts:
  the portal on Linux); `MORPH_TEST_PICK_OPEN` / `MORPH_TEST_PICK_DIR` answer
  them in tests. "Paste from clipboard" beside the words and link fields.
- Device linking: the new device already showed the QR and link; the adding
  side (the screen m3talsmith saw) now says where the link comes from and has
  Paste. A unit test decodes the QR codes with rqrr.
- Tab / Shift+Tab move between the panel's fields (`TextInputEvent::Tab`).
  Pages already moved focus on Tab; the Xvfb checks now include a password
  field.
- macOS: a Cmd shortcut pressed while a page has the keyboard came to the
  shell as `shift-cmd-l` and matched nothing; strokes are compared in one
  modifier order and Cmd maps to the Ctrl binding (`page_action`). An
  Identity button sits in the toolbar. `desktop-smoke.py` opens the panel on
  macOS and Windows (keystroke via System Events / SendKeys, soft when the
  runner cannot send keys, and via `MORPH_TEST_ACTIONS`).
- Linux default browser: Morph writes `mimeapps.list` and every existing
  `<desktop>-mimeapps.list` itself (xdg-settings failed on Hyprland), then
  checks each type with `xdg-mime`.

## Release (0.2.0)

- Release `v0.2.0` (tag at `1002011`, cut with `weirdnet_release.py --version 0.2.0`
  from `6d546b8`): https://github.com/WeirdBrains/morph/releases/tag/v0.2.0. Not a
  pre-release. Library sync (bookmarks, history, settings) on weirdnet-sync 0.2.0,
  Linked devices with revoke, the discovery switch and manual peers (desktop and
  phone), the downloads button and panel, and download sessions for resume.
- The 11 matrix assets come from CI run 44376962078 (`1002011`, all green; iOS
  green) and `... assets 0.2.0` passed. A release arm64 APK
  (`morph-0.2.0-android-arm64.apk`, fixed test key `c2af967c...`) was added by the
  new on-demand `Android release APK` workflow (run 44385402732), and SHA256SUMS
  was regenerated to list it.
- `morph-bin` 0.2.0-1 / `morph` / nix point at the 0.2.0 tarball (sha256
  `c1cd722a7834fd7bee528074777d43cab32fa74992bf9d41dd89c8cb54b4a2d2`);
  `taur-check` is clean. Not uploaded to TAUR: the repo is private (shared
  AGENTS.md) and there is no taurctl on the box. No crates published
  (`[crates] publish = []`).

## Release and TAUR (alpha.15)

- Pre-release `v0.2.0-alpha.15` (tag at `f8aeff6`, cut from `1253118`):
  https://github.com/WeirdBrains/morph/releases/tag/v0.2.0-alpha.15. It
  brings weirdnet-net 0.4 (relays reached over all their addresses, with a
  round trip in Settings), `morph://` pages, `weirdnet-<app>://` links that
  never search (App call not supported yet), and the phone search-engine
  selector beside the URL. All 12 assets come from CI run 44090441120
  (`f8aeff6`, all green; iOS 44090441192 green), and `... assets
  0.2.0-alpha.15` passed.
- `app/pubspec.yaml` `0.2.0-alpha.15+10404`. The APK reports versionName
  0.2.0-alpha.15 and versionCode 10404, and is signed by the fixed test key
  (certificate SHA-256
  `c2af967c4775c926f6db7f051be293821f7b8c4c5b3596c3745ae72fbc8bba51`).
- `morph-bin` 0.2.0alpha15-1: tarball sha256
  `131730c5a79b77c65bd9007aefe06c2946ad181e39dc18e1018bb3f92409aedf` (matches
  the release's SHA256SUMS). Both `.SRCINFO` files match the script's
  generator, and `taur_check` is clean. The archive
  `/workspace/dist/morph-bin-0.2.0alpha15.tar.gz` was made with `taur_pack`.
  It was not uploaded (no taurctl).

## Release and TAUR (alpha.14)

- Pre-release `v0.2.0-alpha.14` (tag at `1f6f148`, cut from `4d52abb`):
  https://github.com/WeirdBrains/morph/releases/tag/v0.2.0-alpha.14. It
  brings compact v2 QR invites (110 bytes, QR v9), the page lag fixes
  (select-theming and favicon scripts, link nodes off the UI thread, QR
  encoded once), the address bar clip and the phone scanner improvements.
  CPU page drawing on Wayland is opt-in (`MORPH_PAGE_CPU=1`). All 12 assets
  come from CI run 43995632824 (`1f6f148`, all green; iOS 43995632872
  green), and `... assets 0.2.0-alpha.14` passed.
- `app/pubspec.yaml` `0.2.0-alpha.14+10403`. The APK reports versionName
  0.2.0-alpha.14 and versionCode 10403, and is signed by the fixed test key
  (certificate SHA-256
  `c2af967c4775c926f6db7f051be293821f7b8c4c5b3596c3745ae72fbc8bba51`).
- `morph-bin` 0.2.0alpha14-1: tarball sha256
  `2d536fa65d0ef652665ec7bb467f98758e0d0d19a144aa5a83b1032d09bfd48f` (matches
  the release's SHA256SUMS). Both `.SRCINFO` files match the script's
  generator, and `taur_check` is clean. The archive
  `/workspace/dist/morph-bin-0.2.0alpha14.tar.gz` was made with `taur_pack`.
  It was not uploaded (no taurctl).

## Release and TAUR (alpha.13)

- Pre-release `v0.2.0-alpha.13` (tag at `0c679c5`, cut from `1b1471d`):
  https://github.com/WeirdBrains/morph/releases/tag/v0.2.0-alpha.13. It
  brings relay status to Settings and the link screens, with Check now. All
  12 assets come from CI run 43972546521 (`0c679c5`, all green), and
  `... assets 0.2.0-alpha.13` passed.
- `app/pubspec.yaml` `0.2.0-alpha.13+10402`. The APK reports versionName
  0.2.0-alpha.13 and versionCode 10402, and is signed by the fixed test key
  (certificate SHA-256
  `c2af967c4775c926f6db7f051be293821f7b8c4c5b3596c3745ae72fbc8bba51`).
- `morph-bin` 0.2.0alpha13-1: tarball sha256
  `20ff35379237d22df51cde6016f1395ebfb03f26512570fd2f326209566ad496` (matches
  the release's SHA256SUMS). Both `.SRCINFO` files match the script's
  generator, and `taur_check` is clean. The archive
  `/workspace/dist/morph-bin-0.2.0alpha13.tar.gz` was made with `taur_pack`.
  It was not uploaded (no taurctl).

## Release and TAUR (alpha.12)

- Pre-release `v0.2.0-alpha.12` (tag at `1003d22`):
  https://github.com/WeirdBrains/morph/releases/tag/v0.2.0-alpha.12, created
  with `python3 scripts/weirdnet_release.py github 0.2.0-alpha.12` and checked
  with `... assets 0.2.0-alpha.12` (12 assets). Every asset comes from CI run
  43951455162 (`1003d22`, every job green on the first attempt), including
  the Linux tarball, zip, debs and rpms (artifact `morph-linux-x86_64`).
- `app/pubspec.yaml` went to `0.2.0-alpha.12+10401` (the cut added one to
  the build number). The APK reports versionName 0.2.0-alpha.12, versionCode
  10401, `org.weirdbrains.morph`. It is signed by the fixed test key
  (`CN=Morph test (not for release), O=WeirdBrains`, certificate SHA-256
  `c2af967c4775c926f6db7f051be293821f7b8c4c5b3596c3745ae72fbc8bba51`).
- `morph-bin` 0.2.0alpha12-1: tarball sha256
  `e09a689eecc07c1f9d2dc1b9f75d749927f232bca436d47437fd1cca89946837` (matches
  the release's SHA256SUMS). Both `.SRCINFO` match `makepkg --printsrcinfo`.
  It was built from the release download, installed and run
  (`morph 0.2.0-alpha.12`) in `archlinux:latest`. Archive:
  `/workspace/dist/morph-bin-0.2.0alpha12.tar.gz`. Not uploaded (no taurctl).

## Release and TAUR (alpha.11)

- Pre-release `v0.2.0-alpha.11` (tag at `dd55483`):
  https://github.com/WeirdBrains/morph/releases/tag/v0.2.0-alpha.11, created
  with `python3 scripts/weirdnet_release.py github 0.2.0-alpha.11` and checked
  with `... assets 0.2.0-alpha.11` (12 assets: the 11 as before plus
  `morph-0.2.0-alpha.11-android-debug.apk`, the CI debug APK, now in
  release.toml's [github] files and android [[platforms]] assets). Linux
  tarball, zip and debs built on the box, rpms in a fedora container
  (`MORPH_SKIP_DEB=1`); macOS, Windows and Android assets from CI run
  43898313375 (`dd55483`, every job green on the first attempt). The commit
  before it, `57d365e`, needed two reruns of desktop-linux (flaky: a Wayland
  scale-2 "Up arrow scrolls", then an Xvfb invite QR read-back that decoded
  nothing; neither touched by the change). The `test-2eb5865` pre-release
  and its tag were deleted.
- `morph-bin` 0.2.0alpha11-1: tarball sha256
  `a98e7108bf4aefeb5a2cbffb095548e70dbce0cddb24d22ce29508f7f0b5e551` (matches
  the uploaded asset); both `.SRCINFO` match `makepkg --printsrcinfo`; built
  from the release download, installed and started (`morph --version`) in
  `archlinux:latest`. Archive: `/workspace/dist/morph-bin-0.2.0alpha11.tar.gz`.
  Not uploaded (no taurctl).

## Release and TAUR (alpha.10)

- Pre-release `v0.2.0-alpha.10` (tag at `535d50a`):
  https://github.com/WeirdBrains/morph/releases/tag/v0.2.0-alpha.10, created
  with `python3 scripts/weirdnet_release.py github 0.2.0-alpha.10` and checked
  with `... assets 0.2.0-alpha.10` (11 assets). Linux tarball, zip and debs
  built on the box, rpms in a fedora container (`MORPH_SKIP_DEB=1`); macOS and
  Windows assets from CI run 43847311783 (`535d50a`). That run's first
  desktop-linux attempt failed one Wayland scale-2 ☰ menu check (the check
  measured the page before the window settled; fixed in the check, `ba98566`);
  the rerun of the failed job passed, so every job is green on the tag.
- `morph-bin` 0.2.0alpha10-1: tarball sha256
  `46ade662d6621833fca0a5e89da2c40f995548cd84222a10a6c5752504f8787e` (matches
  the uploaded asset); `.SRCINFO` matches `makepkg --printsrcinfo`; built from
  the release download, installed and started (`morph --version`) in
  `archlinux:latest`. Archive: `/workspace/dist/morph-bin-0.2.0alpha10.tar.gz`.
  Not uploaded (no taurctl).

## Release and TAUR (alpha.9)

- Pre-release `v0.2.0-alpha.9` (tag at `ad3f958`):
  https://github.com/WeirdBrains/morph/releases/tag/v0.2.0-alpha.9, created
  with `python3 scripts/weirdnet_release.py github 0.2.0-alpha.9` and checked
  with `... assets 0.2.0-alpha.9` (11 assets). Linux tarball, zip and debs
  built on the box, rpms in a fedora container (`MORPH_SKIP_DEB=1`); macOS and
  Windows assets from CI run 43787921654 (`ad3f958`, all jobs green, including
  the macOS and Windows Identity-panel smoke checks).
- `morph-bin` 0.2.0alpha9-1: tarball sha256
  `3f11a198e361243a7597f968b96c4533be29bb1f5ef64b77929b174a6df84c94` (matches
  the uploaded asset); both `.SRCINFO` from `makepkg --printsrcinfo`; built,
  installed and started (`morph --version`, `morph.desktop`) in
  `archlinux:latest`. Archive: `/workspace/dist/morph-bin-0.2.0alpha9.tar.gz`.
  Not uploaded (no taurctl).

## Release and TAUR (alpha.8)

- Pre-release `v0.2.0-alpha.8` (tag at `07b44ca`):
  https://github.com/WeirdBrains/morph/releases/tag/v0.2.0-alpha.8, created
  with `python3 scripts/weirdnet_release.py github 0.2.0-alpha.8` (helpers
  0.3.0: `--prerelease`, SHA256SUMS with GitHub's dotted names) and checked
  with `... assets 0.2.0-alpha.8`. Linux assets built on the box as before;
  macOS assets from CI run 43770520363 (`07b44ca`), Windows assets from CI run
  43767845591 (`53d6b48`, the same Rust sources; only CI and harness files
  changed after it).
- `morph-bin` 0.2.0alpha8-1: tarball sha256
  `7a5a71579c28180e17cc31fed7675804b1c1e70f2bf98387ffa9e8031315be7f`; both
  `.SRCINFO` from `makepkg --printsrcinfo`; built, installed and started in
  `archlinux:latest`; `taur-check` passes. Archive:
  `/workspace/dist/morph-bin-0.2.0alpha8.tar.gz`. Not uploaded (no taurctl).

## Release and TAUR (alpha.7)

- Pre-release `v0.2.0-alpha.7` on GitHub, from `main`, with the tarball, the
  zip, the four debs/rpms and `SHA256SUMS` (dotted asset names).
  Container-checked like alpha.6 (`morph-bin` from the release tarball,
  sha256 matched, installed, `morph --version`, desktop entry; both
  `.SRCINFO` regenerated).
- `packaging/arch/morph-bin` and `packaging/arch/morph` are `0.2.0alpha7`,
  `packaging/nix/release.json` names the same tarball. Recipe archive:
  `/workspace/dist/morph-bin-0.2.0alpha7.tar.gz` (older `SHA256SUMS-*` kept).
  m3talsmith installs with `yay --aururl https://pkg.taur.weirdbrains.com -S morph-bin`.

## Release and TAUR (phase 6)

- Pre-release `v0.2.0-alpha.6` on GitHub, from `main`, with the tarball, the
  zip, the four debs/rpms and `SHA256SUMS` (dotted asset names, as in alpha.5).
  Container-checked: `morph-bin` built from the release tarball (sha256
  matched), installed, `morph --version` = 0.2.0-alpha.6, desktop entry; both
  `.SRCINFO` regenerated with `makepkg --printsrcinfo`.
- `packaging/arch/morph-bin` and `packaging/arch/morph` are `0.2.0alpha6`
  (new optdepends `gst-plugins-bad` for VA-API), `packaging/nix/release.json`
  names the same tarball. Recipe archive: `/workspace/dist/morph-bin-0.2.0alpha6.tar.gz`
  (older `SHA256SUMS-0.2.0-alpha.N` kept beside it). m3talsmith installs with
  `yay --aururl https://pkg.taur.weirdbrains.com -S morph-bin` once it is in
  TAUR (`taurctl` is his side).

## Release and TAUR (phase 5)

- Pre-release `v0.2.0-alpha.5` on GitHub, from `main`, with the tarball, the
  zip, the four debs/rpms and `SHA256SUMS`.
  GitHub turns `~` in asset names into `.`, so the release's debs and rpms are
  `…0.2.0.alpha.5…` and its `SHA256SUMS` uses those names (the package
  versions inside are still `0.2.0~alpha.5`). Container-checked: `morph-bin`
  built from the release tarball (sha256 matched), installed, `morph
  --version`, desktop entry; both `.SRCINFO` match `makepkg --printsrcinfo`.
- `packaging/arch/morph-bin` and `packaging/arch/morph` are `0.2.0alpha5`
  (`morph-bin` with the tarball's sha256), `packaging/nix/release.json` names
  the same tarball. The recipe archive is `morph-bin-0.2.0alpha5.tar.gz`
  (upload from m3talsmith's side with `taurctl pkg upload
  morph-bin-0.2.0alpha5.tar.gz --org WeirdBrains`, dry run first).

## Release and TAUR (phase 4)

- Pre-release `v0.2.0-alpha.4` on GitHub, from `main`, with
  `morph-0.2.0-alpha.4-x86_64-unknown-linux-gnu.tar.gz` and `SHA256SUMS`.
- `packaging/arch/morph-bin` is `pkgver=0.2.0alpha4` with that tarball's
  sha256; `packaging/arch/morph` is `0.2.0alpha4` too. The recipe archive is
  `morph-bin-0.2.0alpha4.tar.gz` (upload from m3talsmith's side with `taurctl pkg
  upload morph-bin-0.2.0alpha4.tar.gz --org WeirdBrains`, dry run first).

## Release and TAUR (phase 3)

- Pre-release `v0.2.0-alpha.3` on GitHub, from `main`, with
  `morph-0.2.0-alpha.3-x86_64-unknown-linux-gnu.tar.gz` and `SHA256SUMS`.
- `packaging/arch/morph-bin` is `pkgver=0.2.0alpha3` with that tarball's
  sha256; `packaging/arch/morph` is `0.2.0alpha3` too. The recipe archive is
  `morph-bin-0.2.0alpha3.tar.gz` (upload from m3talsmith's side with `taurctl pkg
  upload morph-bin-0.2.0alpha3.tar.gz --org WeirdBrains`, dry run first).

## Release and TAUR (phase 2)

- Pre-release `v0.2.0-alpha.2` on GitHub, from `main`, with
  `morph-0.2.0-alpha.2-x86_64-unknown-linux-gnu.tar.gz` and `SHA256SUMS`.
- `packaging/arch/morph-bin` is `pkgver=0.2.0alpha2` with that tarball's
  sha256; `packaging/arch/morph` is `0.2.0alpha2` too. The recipe archive is
  `morph-bin-0.2.0alpha2.tar.gz` (upload from m3talsmith's side with `taurctl pkg
  upload morph-bin-0.2.0alpha2.tar.gz --org WeirdBrains`, dry run first).

## Release and TAUR (phase 1)

- Pre-release `v0.2.0-alpha.1` on GitHub, from `main`, with
  `morph-0.2.0-alpha.1-x86_64-unknown-linux-gnu.tar.gz` and `SHA256SUMS`. The
  tarball holds `bin/morph`, `share/applications/morph.desktop`, the hicolor
  icons, `LICENSE` and `README.md` (`scripts/linux-dist.sh`).
- `packaging/arch/morph-bin` points at that tarball with its sha256 filled in.
  The recipe archive (`morph-bin/PKGBUILD` + `morph-bin/.SRCINFO`) is
  `morph-bin-0.2.0alpha1.tar.gz`, uploaded from m3talsmith's side with
  `taurctl pkg upload morph-bin-0.2.0alpha1.tar.gz --org WeirdBrains` (dry run
  first). The agent does not run `taurctl`.
- CI on `main`: `check` (fmt, clippy, tests, on Ubuntu 24.04 with the desktop
  libraries), `desktop-linux` (tarball, Xvfb key checks, artifacts) and
  `arch-package` (`.SRCINFO` check, `makepkg`, `pacman -U`, `--version`, start
  under Xvfb, the `.pkg.tar.zst` as an artifact), next to the existing jobs.

## Decisions so far (m3talsmith, 2026-11-14)

- **Desktop moves to GPUI; Flutter stays for mobile.** Linux, macOS and Windows get
  a Rust GPUI shell. Android, iOS and F-Droid keep the Flutter app,
  `morph_webview(_inapp)`, `morph-bridge` and `flutter_rust_bridge`.
- **Linux first.** The GPUI shell has to be fully usable on Linux before any macOS
  or Windows GPUI work. Those two are planned in the docs only.
- **The desktop shell is a component host.** Browsing is one component behind its
  own crates (`morph-browser` over `morph-webview`); chat, notes, projects and AI
  come as components the same way. `PRIORITIES.md` 1 item 2 is the backbone.
- **Dart is not an extension language.** It stays only as the mobile UI language.
  No Dart extension SDK, no Dart to JS/WasmGC, no "Rust first; Dart later".
- **Each phase ends with a Linux build m3talsmith can run, including Arch
  packages**: `morph` built from the checkout (`makepkg -si` or `pacman -U`) and
  a `morph-bin` recipe for his TAUR server, uploaded only after he approves the
  phase.
- `PLAN.md` is not touched. `PRIORITIES.md` got one dated progress note only.

All of it is written up in `ARCHITECTURE.md`: sections 1 (shape), 2.0 (desktop web
view), 2a (components), 3 (crates), and the three 2026-11-14 entries at the end
(the switch, Dart, and the open decisions).

## What the planning spike checked (before phase 1)

On this box (Debian 13, Xvfb, `WAYLAND_DISPLAY` unset, webkit2gtk 2.54.0, Mesa
lavapipe Vulkan, Rust 1.100.0), with the spike in `docs/evidence/gpui-spike/`:

- crates.io `gpui` 0.2.2 (2025-10-22, the newest published) does not build with
  current `libc` (0.2.190+): `xattr` 0.2.3 under its `zed-async-tar` uses
  `libc::ENOATTR`, which Linux no longer has. With `xattr` patched it builds, but
  asking its X11 window for a raw handle panics (`unimplemented!()` in
  `src/platform/linux/x11/window.rs`), so wry cannot attach a child web view.
- GPUI from Zed main (`703b5ac3f4ffe1e87a073123ae56e22f72f095c4`, 2026-11-14,
  crates `gpui` + `gpui_platform`) builds (wgpu 30 renderer; Zed pins Rust 1.98.1)
  and implements the X11 and Wayland handles. wry 0.57.2 accepts only an Xlib
  handle, so the spike passes the same X11 window id as an Xlib handle.
- A GPUI window drew its own toolbar with a webkit2gtk child below it showing a
  local page. `xdotool` clicks and typing reached the page input (one title change
  per keystroke).
- `window.open` reached Morph's handler, which built a GTK popup with a web view
  related to the opener (`NewWindowResponse::Create`). Text typed in the popup's
  field showed with the caret in the field; a button that changes the heading
  repainted it; every keystroke also reached the opener by `postMessage`. That is
  the Flutter popup bug's reproduction (local fixture, no real account), passing.
- wry's default popup handling (`NewWindowResponse::Allow`) unwraps the top level
  as a `gtk::ApplicationWindow` and would panic for a child web view (read in
  `wry-0.57.2/src/webkitgtk/mod.rs`), so Morph must make its own popups.

Not checked: a real Wayland session (the plan uses XWayland there), NVIDIA, HiDPI,
IME, window resizing, real sites, macOS, Windows, mobile builds, and the CPU cost
of pumping GTK from a timer.

## The plan: phases, each ending in a Linux build to test

Every phase passes the quality bar before each commit (`cargo fmt --check`,
`cargo build`, `cargo clippy --all-targets -- -D warnings`, `cargo test`), ships
tests for new behaviour, runs GUI checks only under Xvfb (or a headless
compositor), and is pushed straight to `origin/main` after
`git pull --rebase origin main` (no pull requests, no force-push). Domain logic goes in `morph-*`
crates that test without a display; the GPUI crates stay thin.

**How m3talsmith tests a phase on my-desktop:** see "How m3talsmith builds and
installs on my-desktop" above.

**Every phase also ships Arch packages**, in the shape of m3talsmith's existing
TAUR packages (`mouser-bin`, `muninn-bin`, `muninn-app-bin` under org
WeirdBrains, read from https://pkg.taur.weirdbrains.com on 2026-11-14):

- `packaging/arch/morph/PKGBUILD` builds from the checkout with cargo
  (`--locked --release`) for `makepkg -si`. `depends`: `webkit2gtk-4.1`, `gtk3`,
  `glib2`, `libxkbcommon-x11`, `vulkan-icd-loader`, `hicolor-icon-theme`,
  `glibc`, `gcc-libs`; `makedepends`: `cargo`, `git`, `clang`, `pkgconf`.
- `packaging/arch/morph-bin/PKGBUILD` installs the prebuilt app from a GitHub
  Release, like `muninn-app-bin`: `source_x86_64` (and later `source_aarch64`)
  `$url/releases/download/v$_ver/morph-$_ver-$CARCH-unknown-linux-gnu.tar.gz`
  (`_ver` is the tag's version, `0.2.0-alpha.1`; `pkgver` is `0.2.0alpha1`),
  `sha256sums_*` filled from the release's `SHA256SUMS`, `provides=('morph')`,
  `conflicts=('morph')`, `options=('!strip' '!debug')`, the maintainer line
  and a header comment saying where the tarball comes from.
- Both install `/usr/bin/morph`, `/usr/share/applications/morph.desktop`
  (`Exec=morph %u`, http/https handlers), the icon from `docs/brand` in the
  hicolor theme, `LICENSE` and `README.md`, and each has a `.SRCINFO` made with
  `makepkg --printsrcinfo` next to it. CI fails if
  `makepkg --printsrcinfo | diff - .SRCINFO` shows a difference.
- Checked in an `archlinux:latest` container before a phase is called done:
  `makepkg` as a non-root user, `pacman -U`, `morph --version`, a start under
  Xvfb. CI builds `morph` the same way and attaches the `.pkg.tar.zst`.

```sh
cd packaging/arch/morph && makepkg -si     # from the checkout
sudo pacman -U morph-*-x86_64.pkg.tar.zst  # or the package CI attached
```

**Publishing each phase to TAUR.** The agent tags the phase build as a GitHub
pre-release from `main` (`v0.2.0-alpha.N`) with the tarball and `SHA256SUMS`, fills
the `morph-bin` recipe's version and sums, and leaves the recipe archive
(`morph-bin/PKGBUILD` + `morph-bin/.SRCINFO`, as `morph-bin-<pkgver>.tar.gz`) for
upload from m3talsmith's side with `taurctl pkg upload ... --org WeirdBrains`, dry
run first. The agent does not run `taurctl`. After each push, m3talsmith's checkout
at `~/src/WeirdBrains/morph` on my-desktop (branch `main`) is fast-forwarded
(`git pull --ff-only origin main`). CI also attaches the tarball and the Arch
package to each `main` run.

### Phase 1 — GPUI shell on Linux with one working browser component (done)

Flutter is untouched in this phase; both shells live side by side.

- `morph-ui`: component kinds, `Layout` (slots, sizes, visibility, saved per
  profile as JSON), named actions and a keymap with main's shortcuts
  (Ctrl+T, Ctrl+W, Ctrl+L, Ctrl+R, Ctrl+F, Ctrl+Tab, Ctrl+1..9), theme tokens from
  main's `morph_theme.dart`. Unit tests.
- `morph-core`: the address-bar rule ("phrases are searches", main's
  `omnibox.dart` and `searchUrl`) ported to Rust with main's `omnibox_test.dart`
  cases. The Flutter app keeps its Dart copy until phase 2 moves it onto the
  Rust one.
- `morph-webview`: `WebView` trait, events, the wry backend (X11 shim, GTK pump,
  Morph-made popups, a `WebContext` per profile with
  `morph_core::profiles::webkit_data_dir`), and an in-memory fake.
- `morph-browser`: tabs on `morph_core::tabs::TabSet`, address bar, back,
  forward, reload, stop; history recorded through `morph_core::library`. Tests
  drive it with the fake web view.
- `morph-desktop`: binary `morph`, `MORPH_HOME` honoured, GPUI on X11 when
  `DISPLAY` is set, the component host with the browser in the main slot.
- CI: a `desktop-linux` job (build, clippy, test, release tarball
  `morph-<version>-x86_64-unknown-linux-gnu.tar.gz` with `SHA256SUMS`) and an
  `arch-package` job (`archlinux:latest` container, `.SRCINFO` check,
  `makepkg`, `pacman -U`, package artifact); CI runs on pushes to `main` and
  on pull requests. The Flutter jobs stay as they are for now.
- `packaging/arch/morph` and `packaging/arch/morph-bin` (PKGBUILD + `.SRCINFO`
  each), the `.desktop` file and the icon, checked in an Arch container.
- Xvfb checks: open a page, type into it, open a popup and type into it, switch
  and close tabs, type a phrase and get a search; every key m3talsmith listed, in
  pages and popups (results above).
- **m3talsmith tests:** installs the Arch package (`makepkg -si` or `pacman -U`),
  then browse, tabs, address bar, sign-in popups (no real account needed for the
  check, but he may try his own).

### Phase 2 — Flutter desktop removed, shared logic moved down (done)

Result above ("Phase 2 result"). The plan was:


- Remove `app/linux`, `app/macos`, `app/windows`, `packages/morph_webview_linux`,
  desktop-only Dart (window chrome, `window_manager`, desktop chrome widgets), the
  Flutter `linux` and `windows` CI jobs and `scripts/build-windows.ps1`.
- Move the logic in `morph-bridge` that both shells need (library open and unlock,
  identity, device linking, profile deletion side effects) into a toolkit-neutral
  `morph-services`, with its tests. `morph-bridge` keeps its function signatures,
  so the generated Dart bindings do not change.
- The mobile app moves onto the Rust address-bar rule through the bridge.
- `flutter analyze`, `flutter test`, the `morph_webview` and
  `morph_webview_inapp` package tests and the Android CI job stay green.
- **m3talsmith tests:** the updated Arch package, plus the Android debug APK from CI.

### Phase 3 — Everyday browser parts on Linux (done, `v0.2.0-alpha.3`; downloads skipped)

In `PRIORITIES.md` order as far as they apply: library component (bookmarks with
folders and tags, history, search), profiles (switch, create, rename, delete;
library unlock from the keystore, password prompt otherwise), settings component
(tab position, sidebar), find in page, crash and hang panel per tab, Morph error
pages, frameless window with custom controls (close-only on tiling window
managers), downloads to a chosen folder.
- **m3talsmith tests:** the updated Arch package; daily browsing with bookmarks,
  history and two profiles.

### Phase 4 — Identity and links on Linux (done, `v0.2.0-alpha.4`; page bridge round trip not done)

Identity onboarding (create, restore from words or a backup file), recovery
phrase and backups, device linking (QR offer shown, SAS check), "Make default
browser" (`xdg-settings`), links from the command line into the running instance
(single instance), page bridge round trip.
- **m3talsmith tests:** the updated Arch package; onboarding a fresh profile,
  linking with the phone app, opening links from other apps.

### Phase 5 — Linux packaging and the Wayland question (done, `v0.2.0-alpha.5`; native Wayland not done)

The remaining packages per the collective rules (deb, rpm, Nix flake, and every
format with and without the `.desktop` entry), the shared release script
(`scripts/weirdnet_release.py`, `release.toml`, its `taur` step for `morph-bin`), and, depending on m3talsmith's call
on open decision 1, the native Wayland web view work.
- **m3talsmith tests:** a release-style Arch package and the other formats.

### Phase 6 — Native Wayland (done, `v0.2.0-alpha.6`)

Off-screen WebKit pages drawn by GPUI, the key suite on headless sway, no
vendor checks (DMA-BUF probe, VA-API ranks), GDK_SCALE, popup-close and video
fixes. Details in "Phase 6 result" above.
- **m3talsmith tests:** see "Phase 6 result".

### Phase 7 — macOS and Windows (done, `v0.2.0-alpha.8`)

WKWebView and WebView2 child views through the same `morph-webview` trait,
universal Morph.app (zip, dmg), Windows zip and MSI, CI smoke tests. Details in
"Phase 7" above.
- **m3talsmith tests:** the mac and Windows downloads (unsigned; see
  `docs/install-macos-windows.md`).

### Later — more components

Chat, notes, projects and AI components as their domain crates land; on
macOS/Windows: default-browser registration, Apple Events links, popups as
tracked windows, signing.

## Open decisions for m3talsmith

Listed in full in `ARCHITECTURE.md` (2026-11-14 — Open decisions from the GPUI
switch). Decided for phase 1 (`ARCHITECTURE.md`, 2026-11-14 — Phase 1 decisions):

1. Native Wayland: **native since phase 6** (off-screen pages drawn by GPUI;
   XWayland is the fallback). No GPU-vendor checks (m3talsmith, 2026-11-15).
2. GPUI pin: **Zed repository by commit** (`703b5ac`).
8. GPUI cannot draw over the page: **popovers beside it**.
10. Phase builds: **pre-releases from `main`**, `v0.2.0-alpha.N`, `pkgver`
    `0.2.0alphaN` (sorts correctly before a final 0.2.0).

Still open:

3. Keep or drop the Linux texture/DMA-BUF/hidden-text-layer work (only useful
   later for native Wayland)?
4. `flutter_inappwebview` "other platforms" now means Android and iOS only.
5. App updates: RFW and Dart-patch items now concern mobile only.
6. Flutter engine fork: keep the BSD part, or plan BSD on GPUI?
7. GTK 4 / native X11 item in Priorities 2, given wry is GTK 3.
9. Frameless-window rule on the GPUI window: applied in phase 3 (client
   decorations where the WM supports them, close-only on tiling WMs).

## How to resume

```sh
cd /workspace/morph            # or ~/src/WeirdBrains/morph
git switch main && git pull --rebase origin main
```

Next: the macOS/Windows gaps in "Phase 7"; on Wayland, inline IME preedit, `<select>` menus and
the context menu for off-screen pages. Commit as m3talsmith, run the quality bar before each commit,
`git pull --rebase origin main` before each push, push to `origin/main`; no pull
requests, no force-push, leave the remote `gpui` branch alone. Do not modify
`PLAN.md`; `PRIORITIES.md` gets dated progress notes only. Do not touch
`~/.morph`. Do not print secrets. GUI only under Xvfb with `WAYLAND_DISPLAY`
unset (`scripts/xvfb-check.py` starts its own). Do not run `taurctl`.

## History before the GPUI branch

The 2026-11-13 handoff (Windows release build `d30a211`/`e6c0c5e`, Linux
`Isolate.create` fix `005d01c`, the popup bug report) is in git:
`git show d73e240:docs/handoff.md`. The stash `stash@{0}` and the branches
`gpu-fix` and `linux-persistent-webview-session` mentioned there were left alone.
