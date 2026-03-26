# Codex Fork Changes for Temporal Integration

Summary of changes to the `codex` repo on branch `task/codex-temporal` compared to `upstream/main`. These changes make `codex-rs` internals consumable by an external orchestrator (the `codex-temporal` Temporal harness) without modifying the normal in-process codex path.

---

## 1. Visibility Changes (pub(crate) → pub)

The following modules and types were promoted from `pub(crate)` to `pub` in `codex-rs/core/src/lib.rs`:

- **Modules:** `exec_policy`, `safety`, `tools` (and sub-modules `tools::spec`, `tools::registry`, `tools::router`, `tools::parallel`, `tools::context`, `tools::sandboxing`)
- **Structs:** `Session`, `TurnContext`, `ToolsConfig`, `ToolsConfigParams`
- **Functions:** `Session::get_base_instructions`, `Session::replace_history`, `build_specs` (was `#[cfg(test)]`-only), `get_model_offline_for_tests`, `construct_model_info_offline_for_tests`
- **Re-exports added:** `AnyToolResult`, `ConfiguredToolSpec`, `ToolRegistry`, `ToolRegistryBuilder`, `FunctionCallError`, `ToolInvocation`, `ToolCall`, `ToolCallHandler`, `ToolPayload`, `ToolSpec`, `ResponsesApiTool`, `JsonSchema`, `ModelStreamer`, `ChannelEventSink`, `EventSink`, `RolloutFileStorage`, `StorageBackend`, `SamplingRequestResult`, `try_run_sampling_request`, `TurnDiffTracker`, `SharedTurnDiffTracker`, `ExecApprovalRequirement`, `ExecPolicyDecision`, `ContentItem`, `LocalShellAction`, `LocalShellExecAction`, `LocalShellStatus`, `ResponseItem`, `protocol` (full module), `protocol_config_types`, `AuthProvider`, `ModelsProvider`, `apply_role_to_config`

## 2. New Abstraction Traits

### `AgentSession` (core/src/agent_session.rs)
Minimal trait (`submit(Op)` / `next_event() → Event`) that decouples the TUI from `CodexThread`. The TUI can now be driven by any backend that implements this trait — the in-process path uses `CodexThread`, while Temporal provides a workflow-backed implementation.

### `ModelStreamer` (core/src/client.rs)
Trait abstracting the streaming model interface (`stream()`, `try_switch_fallback_transport()`). Blanket-implemented for the existing `ModelClientSession`. Allows Temporal workflows to inject a streamer that routes calls through activities instead of direct HTTP.

### `ToolCallHandler` (core/src/tools/parallel.rs)
Trait for dispatching tool calls from the agentic loop. The in-process `ToolCallRuntime` implements it (its `handle_tool_call` was renamed to `dispatch_tool_call` internally). Temporal can supply an implementation that runs tools as workflow-local activities.

### `EventSink` (core/src/session_io.rs)
Trait for delivering events from the agentic loop. Default `ChannelEventSink` wraps the existing `async_channel::Sender<Event>`. Temporal provides a buffer-backed sink that appends to durable workflow state.

### `StorageBackend` (core/src/session_io.rs)
Trait for persisting rollout items. Default `RolloutFileStorage` wraps the existing `RolloutRecorder`. Temporal provides in-memory durable storage. `Session::flush_rollout` now delegates to this trait.

### `AuthProvider` (login/src/auth/manager.rs)
Trait exposing `auth_cached()`, `auth()`, `reload()` — the subset the TUI actually uses. Implemented by `AuthManager`; allows lightweight stubs in harness contexts.

### `ModelsProvider` (core/src/models_manager/manager.rs)
Trait exposing `try_list_models()` and `list_collaboration_modes()`. Implemented by `ModelsManager`; allows fixed-model stubs in harness contexts.

## 3. New Modules

### `entropy` (core/src/entropy.rs)
Provides `RandomSource` trait and `EntropyProviders` struct with a task-local `ENTROPY` variable. The in-process path uses `SystemRandomSource` (uuid/rand crates). Temporal workflows inject a deterministic implementation for replay safety. Includes `entropy_uuid()` helper with graceful fallback.

### `session_io` (core/src/session_io.rs)
Contains the `EventSink` and `StorageBackend` traits plus their default implementations (see above).

### `agent_session` (core/src/agent_session.rs)
Contains the `AgentSession` trait (see above).

## 4. Minimal Constructors for External Harnesses

### `Session::new_minimal`
Creates a `Session` with pluggable `EventSink` and `StorageBackend` but no-op/default service-level fields (no MCP, no skills, no real auth, no model client). Used by Temporal workflows that call `try_run_sampling_request` directly.

### `TurnContext::new_minimal`
Creates a `TurnContext` from just a model info and config, defaulting everything else. Used when constructing turns outside the full session lifecycle.

### `Config::for_harness` / `Config::from_toml`
- `for_harness(codex_home)`: No-IO constructor using all defaults.
- `from_toml(cfg, overrides, codex_home, user_instructions)`: Reconstructs config from a pre-parsed `ConfigToml` without file I/O (for deterministic contexts where disk reads are forbidden).

### `ConfigBuilder::build_toml_string`
Loads and merges config layers, returning the effective TOML as a string. Used by Temporal activities that load config on a worker and send it to the workflow.

### `ModelsManager::resolve_from_bundled_catalog`
Resolves `ModelInfo` from the compiled-in model catalog without network or instance state. Used for offline model resolution in harness contexts.

## 5. TUI Changes

### External Session Support
- `App.server` changed from `Arc<ThreadManager>` to `Option<Arc<ThreadManager>>` — all accesses are now guarded with `if let Some(s)`.
- New `App::run_with_session()` entry point bootstraps the TUI from an `AgentSession` + `SessionConfiguredEvent` without `ThreadManager`, auth migration prompts, or resume/fork logic.
- New `run_with_session()` top-level function in `codex_tui` handles terminal lifecycle and delegates to `App::run_with_session`.

### Session Wiring (tui/src/chatwidget/agent.rs)
New `wire_session()` function bridges an `AgentSession` to the TUI's channel protocol — spawns op-forwarding (UI→session) and event-forwarding (session→UI) tasks, and injects the initial `SessionConfigured` event.

### External Agent Browser
New `ExternalAgentBrowser` trait with `list_sessions()` / `switch_to()` / `current_session_id()` for the `/session` picker to show external (e.g. Temporal) sessions. Plumbed through `App` as an optional field.

### `ExternalSessionEntry` / `ExternalSwitchResult`
Data types for the session browser UI.

## 6. Miscellaneous

- `SessionServices.rollout` changed from `Mutex<Option<RolloutRecorder>>` to `Arc<Mutex<Option<RolloutRecorder>>>` for shared ownership with `StorageBackend`.
- `models_manager` stored directly on `App` (extracted from `ThreadManager`) so it's available even when `server` is `None`.
- Non-retryable error handling: client-error `UnexpectedStatus` treated as non-retryable to avoid infinite retries on 4xx.
- Bundled catalog fallback in `Session::new_minimal` for model resolution without network.
