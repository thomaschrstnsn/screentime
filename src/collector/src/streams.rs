//! Ensures the JetStream streams the collector publishes to.
//!
//! The publisher owns its streams: the collector creates OBSERVATIONS and ADJUSTMENTS when
//! they're missing, and never updates an existing one, so collectors on different versions
//! can't flap the config and hand edits survive.

use std::time::Duration;

use anyhow::Context as _;
use async_nats::jetstream::{self, ErrorCode, context::GetStreamErrorKind, stream::Config};

/// Ensures every stream in `streams`, retrying each one that fails every 60 s at warn until
/// all are ensured. Never fails: without the streams, observations and time adjustments
/// keep working over core NATS.
pub async fn ensure_all(jetstream: jetstream::Context, streams: Vec<Config>) {
    let mut pending = streams;
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    while !pending.is_empty() {
        interval.tick().await;
        let mut failed = vec![];
        for wanted in pending {
            let stream = wanted.name.as_str();
            match ensure(&jetstream, &wanted).await {
                Ok(Ensured::Created) => tracing::info!(stream, "created stream"),
                Ok(Ensured::Existed { drift }) if drift.is_empty() => {
                    tracing::info!(stream, "stream exists")
                }
                Ok(Ensured::Existed { drift }) => tracing::warn!(
                    stream,
                    ?drift,
                    "stream exists with a config that differs from ours; leaving it alone \
                     (change it with `nats stream edit`, or delete it and restart)"
                ),
                Err(e) => {
                    tracing::warn!(stream, "ensuring stream failed, retrying in 60 s: {e:#}");
                    failed.push(wanted);
                }
            }
        }
        pending = failed;
    }
}

impl JetStream for jetstream::Context {
    async fn stream_config(&self, name: &str) -> anyhow::Result<Option<Config>> {
        match self.get_stream(name).await {
            Ok(stream) => Ok(Some(stream.cached_info().config.clone())),
            Err(e) => match e.kind() {
                GetStreamErrorKind::JetStream(e)
                    if e.error_code() == ErrorCode::STREAM_NOT_FOUND =>
                {
                    Ok(None)
                }
                _ => Err(e).with_context(|| format!("getting stream {name}")),
            },
        }
    }

    async fn create_stream(&self, config: Config) -> anyhow::Result<()> {
        let name = config.name.clone();
        jetstream::Context::create_stream(self, config)
            .await
            .with_context(|| format!("creating stream {name}"))?;
        Ok(())
    }
}

/// The part of JetStream that ensuring streams needs.
pub trait JetStream {
    /// The config of the stream `name`, or `None` when there is no such stream.
    async fn stream_config(&self, name: &str) -> anyhow::Result<Option<Config>>;
    /// Creates a stream with exactly `config`.
    async fn create_stream(&self, config: Config) -> anyhow::Result<()>;
}

/// How a stream ended up ensured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ensured {
    Created,
    /// The stream already existed. `drift` names each setting that differs from ours.
    Existed {
        drift: Vec<&'static str>,
    },
}

/// Creates the stream `wanted.name` when it's missing.
pub async fn ensure(jetstream: &impl JetStream, wanted: &Config) -> anyhow::Result<Ensured> {
    match jetstream.stream_config(&wanted.name).await? {
        None => {
            jetstream.create_stream(wanted.clone()).await?;
            Ok(Ensured::Created)
        }
        Some(existing) => Ok(Ensured::Existed {
            drift: drift(wanted, &existing),
        }),
    }
}

/// The settings we choose that differ in `existing`. Only the settings listed here are
/// compared; the rest (replicas, duplicate window, …) are left to the server's defaults.
fn drift(wanted: &Config, existing: &Config) -> Vec<&'static str> {
    // The server reports "no cap" as -1, where a config asking for none says 0.
    let cap = |n: i64| n.max(0);
    [
        ("subjects", wanted.subjects != existing.subjects),
        ("retention", wanted.retention != existing.retention),
        ("storage", wanted.storage != existing.storage),
        ("discard", wanted.discard != existing.discard),
        (
            "max_messages",
            cap(wanted.max_messages) != cap(existing.max_messages),
        ),
        (
            "max_bytes",
            cap(wanted.max_bytes) != cap(existing.max_bytes),
        ),
        (
            "max_messages_per_subject",
            cap(wanted.max_messages_per_subject) != cap(existing.max_messages_per_subject),
        ),
        ("max_age", wanted.max_age != existing.max_age),
        ("allow_direct", wanted.allow_direct != existing.allow_direct),
    ]
    .into_iter()
    .filter_map(|(setting, differs)| differs.then_some(setting))
    .collect()
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use super::*;

    /// An in-memory JetStream holding streams by name.
    #[derive(Default)]
    struct FakeJetStream {
        streams: Mutex<HashMap<String, Config>>,
        creates: Mutex<Vec<String>>,
    }

    impl FakeJetStream {
        fn creates(&self) -> Vec<String> {
            self.creates.lock().unwrap().clone()
        }
        fn stream(&self, name: &str) -> Option<Config> {
            self.streams.lock().unwrap().get(name).cloned()
        }
    }

    impl JetStream for FakeJetStream {
        async fn stream_config(&self, name: &str) -> anyhow::Result<Option<Config>> {
            Ok(self.stream(name))
        }

        async fn create_stream(&self, config: Config) -> anyhow::Result<()> {
            self.creates.lock().unwrap().push(config.name.clone());
            self.streams
                .lock()
                .unwrap()
                .insert(config.name.clone(), config);
            Ok(())
        }
    }

    fn adjustments() -> Config {
        Config {
            name: "ADJUSTMENTS".into(),
            subjects: vec!["time.adjusted.*.*".into()],
            max_age: Duration::from_secs(365 * 24 * 60 * 60),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn a_missing_stream_is_created_with_our_config() {
        let jetstream = FakeJetStream::default();

        let ensured = ensure(&jetstream, &adjustments()).await.unwrap();

        assert_eq!(ensured, Ensured::Created);
        assert_eq!(jetstream.stream("ADJUSTMENTS"), Some(adjustments()));
    }

    impl FakeJetStream {
        fn with(config: Config) -> Self {
            let fake = Self::default();
            fake.streams
                .lock()
                .unwrap()
                .insert(config.name.clone(), config);
            fake
        }
    }

    #[tokio::test]
    async fn a_hand_edited_stream_is_left_alone_and_its_drift_reported() {
        let edited = Config {
            subjects: vec!["time.adjusted.>".into()],
            max_age: Duration::from_secs(30 * 24 * 60 * 60),
            max_bytes: 1_000_000,
            ..adjustments()
        };
        let jetstream = FakeJetStream::with(edited.clone());

        let ensured = ensure(&jetstream, &adjustments()).await.unwrap();

        assert_eq!(
            ensured,
            Ensured::Existed {
                drift: vec!["subjects", "max_bytes", "max_age"]
            }
        );
        assert_eq!(jetstream.creates(), Vec::<String>::new());
        assert_eq!(jetstream.stream("ADJUSTMENTS"), Some(edited));
    }

    #[tokio::test]
    async fn a_stream_as_the_server_reports_it_has_no_drift() {
        // The server fills in its defaults: -1 for "no cap", a duplicate window, and so on.
        let as_reported = Config {
            max_messages: -1,
            max_bytes: -1,
            max_messages_per_subject: -1,
            max_message_size: -1,
            num_replicas: 1,
            duplicate_window: Duration::from_secs(120),
            ..adjustments()
        };
        let jetstream = FakeJetStream::with(as_reported);

        let ensured = ensure(&jetstream, &adjustments()).await.unwrap();

        assert_eq!(ensured, Ensured::Existed { drift: vec![] });
    }
}
