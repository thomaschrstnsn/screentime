use std::{collections::HashMap, time::Duration};

use anyhow::{Context, Result, bail};
use clap::Parser;
use futures_util::StreamExt;

use crate::timekpr::{Timekpr, TimekprError, ZbusTimekpr};
use screentime_contract::{Reply, subject};

mod adjust;
mod dbus;
mod observation;
mod streams;
mod timekpr;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// NATS server URL
    #[arg(short, long, default_value = "nats://localhost:4222")]
    nats_url: String,

    /// Comma-separated list of usernames to query and publish
    #[arg(short, long, value_delimiter = ',', num_args = 1.., required = true)]
    users: Vec<String>,

    /// Hostname where this collector is running
    #[arg(long)]
    hostname: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    // Startup failures are fatal; systemd's Restart=on-failure tries again.
    let nats = async_nats::connect(&args.nats_url)
        .await
        .context("connecting to NATS")?;
    let connection = zbus::Connection::system()
        .await
        .context("connecting to the D-Bus system bus")?;
    let timekpr = ZbusTimekpr::new(&connection)
        .await
        .context("creating the Timekpr admin proxy")?;

    tokio::spawn(streams::ensure_all(
        async_nats::jetstream::new(nats.clone()),
        vec![
            observation::observations_stream(),
            adjust::stream::adjustments_stream(),
        ],
    ));
    let observing = tokio::spawn(observe_loop(
        nats.clone(),
        timekpr.clone(),
        args.hostname.clone(),
        args.users.clone(),
    ));

    tokio::select! {
        result = serve_adjustments(nats, timekpr, args.hostname, args.users) => result,
        joined = observing => {
            joined.context("observation loop panicked")?;
            bail!("observation loop ended")
        }
    }
}

/// Publishes an observation of every user every 5 s. A user Timekpr can't report on is
/// skipped for that tick; the others are still published.
async fn observe_loop(
    nats: async_nats::Client,
    timekpr: ZbusTimekpr,
    host: String,
    users: Vec<String>,
) {
    let mut failing: HashMap<String, TimekprError> = HashMap::new();
    let mut interval = tokio::time::interval(Duration::from_secs(5));

    loop {
        interval.tick().await;

        for user in &users {
            match timekpr.observe(user).await {
                Ok(observation) => {
                    if let Some(e) = failing.remove(user) {
                        tracing::info!(user, "observing again after failing with {}", e.kind());
                    }
                    publish_observation(&nats, &host, user, &observation).await;
                }
                Err(e) => {
                    match failing.get(user) {
                        None => tracing::warn!(user, kind = e.kind(), "observation failed: {e}"),
                        Some(_) => {
                            tracing::debug!(user, kind = e.kind(), "observation still failing: {e}")
                        }
                    }
                    failing.insert(user.clone(), e);
                }
            }
        }

        if let Err(e) = nats.flush().await {
            tracing::warn!("flush failed: {e:?}");
        }
    }
}

async fn publish_observation(
    nats: &async_nats::Client,
    host: &str,
    user: &str,
    observation: &observation::Observation,
) {
    let payload = serde_json::to_vec(observation).expect("an observation always serializes");
    match nats
        .publish(subject::observation(host, user), payload.into())
        .await
    {
        Ok(()) => tracing::info!(user, ?observation, "published observation"),
        Err(e) => tracing::error!(user, "publishing observation failed: {e:?}"),
    }
}

/// Serves time adjustments on `time.adjust.<host>.*`, one request at a time in arrival
/// order. Returns only when the subscription ends, which is fatal.
async fn serve_adjustments(
    nats: async_nats::Client,
    timekpr: ZbusTimekpr,
    host: String,
    users: Vec<String>,
) -> Result<()> {
    let subject = subject::adjust(&host, "*");
    let mut requests = nats
        .subscribe(subject.clone())
        .await
        .with_context(|| format!("subscribing to {subject}"))?;
    tracing::info!("serving time adjustments on {subject}");

    while let Some(message) = requests.next().await {
        let Some(user) = message.subject.rsplit('.').next() else {
            continue;
        };
        // Read leniently, so even an invalid request is logged with its id when it has one.
        let request_id = serde_json::from_slice::<serde_json::Value>(&message.payload)
            .ok()
            .and_then(|v| v.get("request_id")?.as_str().map(str::to_owned));
        let (reply, event) = adjust::handle(
            &users,
            &timekpr,
            &host,
            user,
            &message.payload,
            jiff::Timestamp::now(),
        )
        .await;

        match (&reply, &event) {
            (Reply::Ok { .. }, Some(event)) => tracing::info!(?event, "time adjusted"),
            (Reply::Error { code, message }, _) => {
                tracing::warn!(
                    user,
                    ?request_id,
                    ?code,
                    "time adjustment failed: {message}"
                )
            }
            _ => {}
        }

        // Reply first, then the observation, then the event; a failed publish is only logged.
        match &message.reply {
            Some(reply_to) => {
                let payload = serde_json::to_vec(&reply).expect("a reply always serializes");
                if let Err(e) = nats.publish(reply_to.clone(), payload.into()).await {
                    tracing::error!(user, "publishing reply failed: {e:?}");
                }
            }
            None => tracing::warn!(user, "time adjustment request had no reply subject"),
        }
        if let Reply::Ok { observation, .. } = &reply {
            publish_observation(&nats, &host, user, observation).await;
        }
        if let Some(event) = &event {
            let payload = serde_json::to_vec(event).expect("an event always serializes");
            if let Err(e) = nats
                .publish(subject::adjusted(&host, user), payload.into())
                .await
            {
                tracing::error!(user, "publishing time.adjusted failed: {e:?}");
            }
        }
        if let Err(e) = nats.flush().await {
            tracing::warn!("flush failed: {e:?}");
        }
    }

    bail!("the {subject} subscription ended")
}
