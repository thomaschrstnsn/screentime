pub use screentime_contract::Observation;
use screentime_contract::{OBSERVATIONS_STREAM, subject};

/// The JetStream stream holding the latest observation of every monitored user, which
/// web reads.
pub fn observations_stream() -> async_nats::jetstream::stream::Config {
    use async_nats::jetstream::stream::{Config, DiscardPolicy, StorageType};
    Config {
        name: OBSERVATIONS_STREAM.into(),
        subjects: vec![subject::observation("*", "*")],
        max_messages_per_subject: 1,
        discard: DiscardPolicy::Old,
        allow_direct: true,
        storage: StorageType::File,
        ..Default::default()
    }
}
