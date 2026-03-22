use tracing::level_filters::LevelFilter;
use tracing_subscriber::Layer;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub fn init(verbose: bool) {
    let level = if verbose {
        LevelFilter::DEBUG
    } else {
        LevelFilter::INFO
    };

    let _ = tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_target(false)
                .with_writer(std::io::stderr)
                .with_filter(level),
        )
        .try_init();
}
