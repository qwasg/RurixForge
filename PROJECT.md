# Project: Antigravity Subscription Reverse Proxy in RurixForge

## Architecture
- **Backend Module (`crates/forge-agentd` & `crates/gend`)**:
  - `src/antigravity.rs`: Central module for Antigravity channel persistence (`data/llm-antigravity.json`), DPAPI keystore integration (`gend::keystore::set_key("antigravity", ...)`), quota probe client (`/v1/models` & rate limits extraction), Axum REST handlers, and `antigravity_step` for SSE streaming completions.
  - `src/modelspec.rs`: Model card registrations for `gemini-3.8-flash` and `gemini-3.8-pro` with thinking capability, reasoning effort tiers, and 1M context options.
  - `src/snapshot.rs`: Availability injection into `GET /api/forge/design-snapshot` (`available` / `needs-config` / `disconnected`).
  - `src/llm.rs`: `Provider::Antigravity` and `Provider::AntigravityNotConfigured`, non-transient error mapping for immediate fail-fast on turn 1 (`ANTIGRAVITY_NOT_CONFIGURED`), and step dispatch.
  - `src/agent.rs`: Explicit provider mapping in `provider_for_session`, native MCP tool mounting, driving turns entirely within RurixForge's in-process `run_tool_loop_inner` without external agent delegation.
  - `src/main.rs`: Axum route registrations under `/api/forge/llm/antigravity/*`.
- **Frontend Module (`packages/client`)**:
  - `src/lib/forgeApi.ts`: Type contracts and BFF client functions (`getAntigravityStatus`, `postAntigravityConfig`, `postAntigravityProbe`).
  - `src/components/settings/ModelsPage.tsx`: `AntigravityCard` in BYO section with status badges, latency badge, quota `RateLimitBar`, configuration drawer, presets, and Test Connection button.
  - `src/components/chat/ModelPicker.tsx`: Antigravity model group display, `needs-config` status handling, thinking toggle, reasoning effort selector, and 1M context denominator.

## Feature Inventory
| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | Antigravity Config & Keystore Persistence | Persist `baseUrl`, `model`, `enabled` to `data/llm-antigravity.json`; store token in keystore securely under "antigravity"; no key leakage | M1 | R1.1 |
| 2 | Antigravity REST BFF Endpoints | `GET /status`, `POST /config`, `POST /probe` with input validation and key sanitization | M1 | R1.2 |
| 3 | Quota Probe & Snapshot Availability | Probe endpoint latency & rate limits (primary/secondary windows, resetsAt), inject availability into `design-snapshot` | M1 | R1.3 |
| 4 | Provider Enum & Turn-1 Fail-Fast | Add `Provider::Antigravity` & `AntigravityNotConfigured`; fail turn 1 with `ANTIGRAVITY_NOT_CONFIGURED` without retry | M1 | R2.1 |
| 5 | Native In-Process Agent Loop Integration | `antigravity_step` invokes reverse proxy streaming completions; RurixForge in-process `run_tool_loop_inner` drives MCP tools; zero external agent delegation | M1 | R2.2 |
| 6 | Modelspec Registry & Thinking Options | Register `gemini-3.8-flash` & `gemini-3.8-pro` with Thinking, Reasoning Effort, and 1M context options in `modelspec.rs` | M1 | R2.3 |
| 7 | Frontend ModelsPage Antigravity Card | Dedicated channel card with status badges (`available`, `needs-config`, `offline`), latency probe, and RateLimitBar | M2 | R3.1-2 |
| 8 | Frontend Configuration Drawer & Probe UI | Edit drawer for baseUrl, model presets, API Key, and interactive "Test Connection" button with live feedback | M2 | R3.3 |
| 9 | Chat Composer & Model Switcher | Antigravity group in ModelPicker, unconfigured state disabling, thinking toggle, effort tiers, and context window slider | M2 | R3.4 |
| 10 | Comprehensive Test Suite & Regression Verification | Rust unit tests in `forge-agentd` (config, keystore, provider, failure codes, snapshot) & Frontend typecheck/tests | M3 | AC |

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| M1 | Backend Core (`forge-agentd`) | Antigravity config, keystore, BFF routes, quota probe, snapshot, native provider, modelspec, fail-fast turn 1, and cargo tests | none | DONE |
| M2 | Frontend Client (`packages/client`) | AntigravityCard, RateLimitBar, settings drawer, probe feedback, ModelPicker Antigravity group, thinking/effort/context tiers, and client tests | M1 | DONE |
| M3 | Comprehensive Verification & Hardening | Full backend & frontend test suites, production build validation, and forensic integrity audit | M1, M2 | DONE |

## Interface Contracts
### BFF ↔ Client (`/api/forge/llm/antigravity/*`)
- `GET /api/forge/llm/antigravity/status`:
  - Response: `{ configured: bool, baseUrl: string, model: string, keyConfigured: bool, quota?: AntigravityQuota, rateLimits?: AntigravityQuota, latencyMs?: number, models?: string[] }`
  - Redline R-5: Secret key is NEVER returned in response.
- `POST /api/forge/llm/antigravity/config`:
  - Request: `{ baseUrl: string, model: string, key?: string }`
  - Response: `{ configured: bool, baseUrl: string, model: string, keyConfigured: bool }` (400 if baseUrl or model is empty).
- `POST /api/forge/llm/antigravity/probe`:
  - Response: `{ ok: bool, latencyMs?: number, error?: string, quota?: AntigravityQuota, rateLimits?: AntigravityQuota, models?: string[] }`
  - `AntigravityQuota`: `{ primary?: { usedPercent: number, remainingPercent?: number, resetsAt?: string }, secondary?: { usedPercent: number, remainingPercent?: number, resetsAt?: string } }`

### Modelspec ↔ Session (`modelspec.rs` ↔ `agent.rs` ↔ Client)
- Model IDs: `"antigravity/gemini-3.8-flash"`, `"antigravity/gemini-3.8-pro"`, also aliased to `"gemini-3.8-flash"`, `"gemini-3.8-pro"`.
- Group: `"Antigravity"`.
- Context Options: includes `"1m"` (1,048,576 tokens).
- Thinking: `supports_thinking: true`, `effort_options: EFFORTS_FULL`.

## Code Layout
- `crates/forge-agentd/src/antigravity.rs`: Configuration, keystore access, probe, routes, step implementation, and unit tests.
- `crates/forge-agentd/src/main.rs`: Route mounting for `/api/forge/llm/antigravity/*`.
- `crates/forge-agentd/src/snapshot.rs`: Snapshot model list and availability injection.
- `crates/forge-agentd/src/modelspec.rs`: Model cards registration.
- `crates/forge-agentd/src/llm.rs`: `Provider` enum variants, `is_transient_llm_error`, `failure_code_from_error`, `step_for_provider`.
- `crates/forge-agentd/src/agent.rs`: `provider_for_session`, MCP tool registration, unit test for fail-fast.
- `packages/client/src/lib/forgeApi.ts`: Antigravity API types and methods.
- `packages/client/src/components/settings/ModelsPage.tsx`: Antigravity channel card, quota progress bar, and config drawer.
- `packages/client/src/components/chat/ModelPicker.tsx`: Model selection, unconfigured state disabling.
- `packages/client/test/antigravitySettings.test.tsx`: Frontend unit tests for Antigravity card & quota.
