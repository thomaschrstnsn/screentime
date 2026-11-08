# NATS authorization for screentime-collector / screentime-web

Research for issue #4 (child of #1, "time adjustments from the web UI").
Researched 2026-11-08. Sources are docs.nats.io (current site), the nats-server
source on `main` (2.16.0-dev), and the async-nats 0.46.0 / 0.48.0 crate source.
Each claim below cites the source it came from.

## Setup and threat

- nats-server runs in docker on **enix**, JetStream on, stream `OBSERVATIONS` on `time.obs.>`.
  It is presumed to have no auth today, so every client lands in the global account `$G`.
- **screentime-collector** (async-nats 0.46, host cyrus) uses a core `publish` to
  `time.obs.<host>.<user>` (`src/main.rs:88`, not a JetStream publish). It will also serve
  request-reply on `time.adjust.<host>.<user>` and publish `time.adjusted.<host>.<user>`.
- **screentime-web** (async-nats 0.48) calls `jetstream.get_stream(OBSERVATIONS)`, then
  `stream.create_consumer(pull::Config { durable_name: None, filter_subject, deliver_policy: LastPerSubject })`,
  then `consumer.messages()`, and calls `message.ack()` on each message
  (`~/src/screentime-web/src/main.rs:120-205`). That is an **ephemeral pull consumer with no name**.
  It will also send requests to `time.adjust.>`.
- Threat: the monitored user (a kid) on cyrus must not be able to publish to `time.adjust.>`.

## 1. Which mechanism is proportionate?

| Mechanism | Server holds | Secret on the wire? | Per-client permissions? | Verdict |
|---|---|---|---|---|
| Token | one global token | yes | **no**: `token` is a global value, "exclusive of `user` and `password`" ([authorization ref](https://docs.nats.io/reference/config/authorization)) | Reject. It can't tell web apart from collector. |
| Username/password | bcrypt hash (from `nats server passwd`) ([users ref](https://docs.nats.io/reference/config/authorization/users)) | **yes**, plaintext unless TLS | yes | OK |
| NKey | the **public** key only (`U...`), "Exclusive with `username` and `password`" ([users ref](https://docs.nats.io/reference/config/authorization/users)) | **no**: the client signs a server nonce ([auth basics](https://docs.nats.io/learn/security/authentication-basics)) | yes | **Recommended** |
| JWT / operator mode (`.creds`) | operator/account JWTs, resolver | no | yes | Too much here. Needs `nsc`, an operator, accounts and a resolver, and it rules out `no_auth_user` ("no_auth_user not compatible with Trusted Operator", nats-server `auth.go` `validateNoAuthUser`). The docs pitch it for "when a single config file no longer scales" ([auth basics](https://docs.nats.io/learn/security/authentication-basics)). |

**Recommendation:** a static `authorization { users = [...] }` block using **NKey users**,
with **no `accounts` block**.

- With NKeys the server config holds only public keys. It is not secret, so it can live in
  the nix store or a plain bind mount. No agenix is needed on enix.
- Only the clients hold secrets: the seed files, delivered by agenix to cyrus and to the
  web host.
- The seed never crosses the LAN, which matters because there is no TLS today.
- Bcrypt passwords are the fallback if you don't want to deal with nkey tooling. They are
  fine too, but the password goes over the LAN in clear text unless you add TLS.

**Why no `accounts` block:** once you declare accounts, "connections no longer land in
`$G` by default". JetStream also becomes opt-in per account, and "a stream created back in
`$G` before the accounts existed doesn't follow" into the new account
([accounts](https://docs.nats.io/running-a-nats-service/configuration/securing_nats/accounts)).
Users listed under the top-level `authorization` block stay in `$G`, so the existing
`OBSERVATIONS` stream keeps working.

## 2. Permission syntax and `allow_responses`

- Each user has `permissions { publish {allow, deny}, subscribe {allow, deny} }`. A bare
  subject or list means allow-only, which "implicitly denies publishing to all other
  subjects" ([permissions ref](https://docs.nats.io/reference/config/authorization/users/permissions)).
- "When a subject matches both lists, deny wins." `*` matches one token and `>` matches one
  or more trailing tokens. `default_permissions` applies only to users with no
  `permissions` block of their own, and the two are never merged
  ([authorization](https://docs.nats.io/running-a-nats-service/configuration/securing_nats/authorization)).
- A denied JetStream API call shows up as a **timeout**, not a permission error, "because
  every JetStream API call is a request" ([authorization](https://docs.nats.io/learn/security/authorization)).
  Keep this in mind when debugging.
- **`allow_responses`** lets a responder publish to the reply subject of a request it
  received, without a publish grant on `_INBOX.>`. The server "tracks each reply subject it
  handed out and permits exactly that one reply" ([authorization](https://docs.nats.io/learn/security/authorization)).
  - Forms: `allow_responses: true`, or `allow_responses: { max: N, expires: "1m" }`
    ([allow_responses ref](https://docs.nats.io/reference/config/authorization/users/permissions/allow_responses)).
  - Defaults, from **source**: `DEFAULT_ALLOW_RESPONSE_MAX_MSGS = 1` and
    `DEFAULT_ALLOW_RESPONSE_EXPIRATION = 2 * time.Minute` (nats-server `server/const.go`).
    The reference page says the expiry "doesn't have a time limit", which **contradicts the
    source**. Trust the source.
  - Side effect: if `allow_responses` is set and there is no `publish` allow list, the server
    sets `Publish.Allow = []`, a blanket deny for everything except responses
    (`server/opts.go`, `parsePermissionsValue`: "We turn off the blanket allow statement").
    So you must list the collector's normal publish subjects explicitly.

## 3. Exact JetStream / inbox permissions

API subjects and their format come from nats-server `server/jetstream_api.go` and
`server/consumer.go`. Client behaviour comes from the async-nats 0.48.0 source.

What screentime-web's code path sends:

| Step | Client call (async-nats 0.48) | Subject |
|---|---|---|
| `get_stream("OBSERVATIONS")` | `jetstream/context.rs:697` | request `$JS.API.STREAM.INFO.OBSERVATIONS` |
| `create_consumer` with no `name`/`durable_name` | `context.rs:1499-1500`: with no name it uses `CONSUMER.CREATE.<stream>` and leaves the filter out of the subject | request `$JS.API.CONSUMER.CREATE.OBSERVATIONS` |
| `consumer.messages()` pulls batches | `consumer/pull.rs` `"{}.CONSUMER.MSG.NEXT.{}.{}"` | publish `$JS.API.CONSUMER.MSG.NEXT.OBSERVATIONS.<server-generated name>` with reply `_INBOX.…` |
| heartbeat-miss recovery | `pull.rs:961` `consumer.get_info()` | request `$JS.API.CONSUMER.INFO.OBSERVATIONS.<name>` |
| `message.ack()` | `jetstream/message.rs:286` publishes to the message's reply subject | publish `$JS.ACK.OBSERVATIONS.<name>.…` (v1 format `$JS.ACK.%s.%s`. The v2 format `$JS.ACK.<domain>.<acchash>.…` is behind a server feature flag) |
| replies / delivered msgs | `client.new_inbox()` = `"{inbox_prefix}.{nuid}"`, default prefix `_INBOX` | subscribe `_INBOX.>` |

The consumer name is server-generated, so the `MSG.NEXT` and `INFO` grants need a `*`.
With `durable_name: Some("web")`, the create subject would be
`$JS.API.CONSUMER.CREATE.OBSERVATIONS.web.time.obs.>` and the other grants could name `web`
exactly.

**Publisher into the stream (collector):** it uses core `publish`, so it needs only publish
on `time.obs.cyrus.>`. A JetStream `publish().await` with an ack would also need subscribe on
`_INBOX.>`, because the PubAck comes back as a reply.

**Tightening `_INBOX.>`:** each client can set `ConnectOptions::custom_inbox_prefix("_INBOX_web")`
(present in 0.46 and 0.48), and then you grant subscribe only on `_INBOX_web.>`. That stops one
client snooping on another client's replies. It is optional at this scale.

## 4. Proposed server config

```hcl
# nats-server.conf on enix (mounted into the container)
jetstream { store_dir: /data }

authorization {
  users = [
    # screentime-collector on cyrus
    { nkey: "UCOLLECTOR_CYRUS_PUBLIC_KEY..."
      permissions: {
        publish:   { allow: ["time.obs.cyrus.>", "time.adjusted.cyrus.>"] }
        subscribe: { allow: ["time.adjust.cyrus.>"] }
        allow_responses: { max: 1, expires: "30s" }
      }
    }
    # screentime-web
    { nkey: "UWEB_PUBLIC_KEY..."
      permissions: {
        publish: { allow: [
          "time.adjust.>",                                  # adjustment requests
          "$JS.API.STREAM.INFO.OBSERVATIONS",
          "$JS.API.CONSUMER.CREATE.OBSERVATIONS",          # ephemeral, unnamed
          "$JS.API.CONSUMER.CREATE.OBSERVATIONS.>",        # in case it gets a name/durable later
          "$JS.API.CONSUMER.MSG.NEXT.OBSERVATIONS.*",
          "$JS.API.CONSUMER.INFO.OBSERVATIONS.*",
          "$JS.ACK.OBSERVATIONS.>"
        ] }
        subscribe: { allow: ["_INBOX.>", "time.adjusted.>", "time.obs.>"] }
      }
    }
    # admin, for the `nats` CLI from a trusted machine
    { nkey: "UADMIN_PUBLIC_KEY..." }   # no permissions block = everything
  ]
}
# no no_auth_user: anonymous connects are rejected
```

A collector on another host gets its own nkey user with that host in its subjects. That
way a compromised host can't adjust another host's time.

**Kid threat:** the collector's seed lives on cyrus. Use agenix's default `owner = root`,
`mode = 0400`, or an owner of the collector's service user. The collector user also cannot
publish to `time.adjust.>`. An anonymous connection is rejected outright. So the kid needs
root on cyrus or the web host's seed.

## 5. Loading credentials in async-nats (0.46 and 0.48)

Both versions expose the same `ConnectOptions` builders
([docs.rs 0.48.0](https://docs.rs/async-nats/0.48.0/async_nats/struct.ConnectOptions.html),
[docs.rs 0.46.0](https://docs.rs/async-nats/0.46.0/async_nats/struct.ConnectOptions.html)):

| Credential | API | Takes |
|---|---|---|
| NKey | `.nkey(seed: String)` | the **seed string** (`SU…`), not a path, so read the agenix file yourself |
| user/password | `.user_and_password(user, pass)` | strings |
| token | `.token(token)` | string |
| JWT creds | `async .credentials_file(path)` / `.credentials(&str)` | path / file contents |

The `nkeys` feature is on by default (async-nats `Cargo.toml` `default = [... "nkeys" ...]`).

- **async-nats reads no credential env vars** (there is no `NATS_CREDS` etc.).
- **It ignores `nats://user:pass@host` in the URL.** `ServerAddr::username()`/`password()`
  exist, but the connector builds `CONNECT` only from `options.auth`
  (`connector.rs:631-632` in 0.48, `:219` in 0.46). So the current `NATS_URL` env/flag can't
  carry credentials.

Each app needs a small change: a `--nats-nkey-file` flag (or `NATS_NKEY_FILE` env) that does

```rust
let mut opts = async_nats::ConnectOptions::new();
if let Some(path) = &args.nats_nkey_file {
    let seed = std::fs::read_to_string(path)?.trim().to_string();
    opts = opts.nkey(seed);
}
let client = opts.connect(&args.nats_url).await?;
```

Generate keys with the `nk` tool from nats-io/nkeys (`nk -gen user -pubout`), or with
`nsc generate nkey --user`. The public key goes in the server config and the seed goes into
agenix.

## 6. `no_auth_user`: keeping an anonymous user alongside authenticated ones

- `no_auth_user: <name>` makes unauthenticated clients take on that user's permissions.
  "This must be a user defined in either the `authorization` or `accounts` block"
  ([no_auth_user ref](https://docs.nats.io/reference/config/no_auth_user)). The server source
  also accepts an **nkey public key** as the value (`auth.go` `validateNoAuthUser`).
- You can't add or change it by config reload. Removing it reloads. So adding it needs a
  container restart ([no_auth_user ref](https://docs.nats.io/reference/config/no_auth_user)).
  `nats-server -t` does not catch a missing user
  ([accounts](https://docs.nats.io/running-a-nats-service/configuration/securing_nats/accounts)).
- It is incompatible with operator mode (`auth.go`).

So **yes**, an anonymous read-only user is possible, but only for **core** subscriptions.
JetStream reads need publish on `$JS.API.…`, so "read-only" would mean live `time.obs.>`
only:

```hcl
authorization {
  users = [
    ...,
    { user: "anon", password: "unused-random-string"
      permissions: { publish: { deny: [">"] }, subscribe: { allow: ["time.obs.>"] } } }
  ]
}
no_auth_user: anon
```

**Recommendation:** leave `no_auth_user` unset. Use it only as a temporary migration aid
while the two apps get nkey support. To do that, point it at a user that has the
**current** effective needs: publish `time.obs.>`, plus web's JetStream grants from
section 3. Never give it `time.adjust.>`. Remove it once both clients authenticate.

## Migration order

1. Add nkey-file support to both apps (section 5). Deploy them while the server still
   allows anonymous clients. Sending creds to a server without auth is harmless.
2. Generate the nkeys, add the seeds to agenix, and wire them into the services.
3. Restart the nats container with the `authorization` block above.
4. Check with `nats --nkey <seedfile> pub time.adjust.cyrus.kid x` using the collector's seed.
   Expect a permissions violation.
