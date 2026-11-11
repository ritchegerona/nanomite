# Herdr integration

nanomite reports its lifecycle to [Herdr](https://herdr.dev) (terminal workspace
manager) when running inside a Herdr pane, so the pane reads as a first-class
`nanomite` agent: the sidebar labels it, `herdr agent list` includes it, and
`herdr agent wait` can watch for `idle` / `blocked`.

## How it works

Detection is automatic. When `HERDR_ENV=1`, `HERDR_PANE_ID` and
`HERDR_SOCKET_PATH` are present at startup, `src/herdr.rs` spawns a
fire-and-forget reporter thread. The UI thread never touches the socket
(invariant 1); reports are best-effort and their failure is invisible.

State transitions reported:

| App state                                        | Herdr state |
| ------------------------------------------------ | ----------- |
| `start_turn` (prompt submitted / queue drains)    | `working`   |
| Confirmer asks `y/a/n/d` for an external tool     | `blocked` (+ tool name) |
| Turn finished, interrupted or failed              | `idle`      |

The same states drive the OSC window title (`nanomite`, `nanomite — working`,
`nanomite — approval needed`), so other multiplexers and Herdr's own screen
detection see the pane too.

## Protocol

One JSON line per report to the integration socket, mirroring Herdr's own
agent plugins (opencode, pi):

```json
{"id":"herdr:nanomite:3","method":"pane.report_agent","params":{
  "pane_id":"w1:p2","source":"herdr:nanomite","agent":"nanomite",
  "state":"working","seq":3,"message":null,"agent_session_id":"nanomite-1773..."}}
```

Verified live against herdr 0.8.2 (2026-11-11): unknown agent ids are accepted
and `herdr agent list` shows the agent with its state.

## Opting out

```toml
# ~/.config/nanomite/config.toml
[herdr]
enabled = false
```

or `NANOMITE_HERDR=0` in the environment (wins over the file). Without Herdr's
env vars nothing happens even when `enabled = true`.

## Screen-detection manifest

`distribution/agent-detection/nanomite.toml` ships the manifest Herdr uses for
screen-based detection. Herdr 0.8.2 only loads built-in agent kinds (unknown
manifests are skipped), so the hook above is the authority today; the manifest
is kept in-repo so future herdr versions that load third-party manifests work
without a code change. Install path when supported:
`~/.local/state/herdr/agent-detection/nanomite.toml`.
