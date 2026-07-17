//! Configures local diagnostics and opt-in redacted OTLP traces.
// Rust guideline compliant 2026-02-21

use std::backtrace::Backtrace;
use std::fmt::{Display, Formatter};
use std::time::Duration;

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{Protocol, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::{Layer, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

const ENABLE_VARIABLE: &str = "REDCROWN_OTEL_ENABLED";
/// A two-second export timeout keeps diagnostics subordinate to desktop exit.
const EXPORT_TIMEOUT: Duration = Duration::from_secs(2);
const SAFE_TELEMETRY_TARGET: &str = "redcrown_telemetry";

/// Owns diagnostics providers that require orderly shutdown.
#[derive(Debug)]
pub struct Diagnostics {
    tracer_provider: Option<SdkTracerProvider>,
}

impl Diagnostics {
    /// Initializes local logging and optional OTLP trace export.
    ///
    /// OTLP export is enabled only when `REDCROWN_OTEL_ENABLED` is `1` or
    /// `true`. Collector configuration follows standard `OTEL_EXPORTER_OTLP_*`
    /// environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error when the global tracing subscriber was already set.
    pub fn initialize() -> Result<Self, DiagnosticsError> {
        if otlp_enabled() {
            match build_tracer_provider() {
                Ok(provider) => initialize_with_otlp(provider),
                Err(error) => {
                    eprintln!(
                        "OpenTelemetry export is disabled because initialization failed: {}",
                        error.user_message()
                    );
                    initialize_local()
                }
            }
        } else {
            initialize_local()
        }
    }

    /// Flushes and stops the optional OTLP provider.
    pub fn shutdown(&mut self) {
        if let Some(provider) = self.tracer_provider.take()
            && let Err(error) = provider.shutdown_with_timeout(EXPORT_TIMEOUT)
        {
            eprintln!("OpenTelemetry shutdown was incomplete: {error}");
        }
    }

    /// Reports whether OTLP export is active.
    #[must_use]
    pub const fn otlp_enabled(&self) -> bool {
        self.tracer_provider.is_some()
    }
}

impl Drop for Diagnostics {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn initialize_local() -> Result<Diagnostics, DiagnosticsError> {
    tracing_subscriber::registry()
        .with(local_layer())
        .try_init()
        .map_err(|error| {
            DiagnosticsError::new(format!("failed to initialize diagnostics: {error}"))
        })?;
    Ok(Diagnostics {
        tracer_provider: None,
    })
}

fn initialize_with_otlp(provider: SdkTracerProvider) -> Result<Diagnostics, DiagnosticsError> {
    let tracer = provider.tracer("redcrown-desktop");
    let telemetry = tracing_opentelemetry::layer()
        .with_tracer(tracer)
        .with_filter(
            Targets::new()
                .with_target(SAFE_TELEMETRY_TARGET, LevelFilter::TRACE)
                .with_default(LevelFilter::OFF),
        );
    tracing_subscriber::registry()
        .with(local_layer())
        .with(telemetry)
        .try_init()
        .map_err(|error| {
            DiagnosticsError::new(format!("failed to initialize diagnostics: {error}"))
        })?;
    Ok(Diagnostics {
        tracer_provider: Some(provider),
    })
}

fn local_layer<S>() -> impl Layer<S>
where
    S: tracing::Subscriber + for<'span> LookupSpan<'span>,
{
    fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
}

fn build_tracer_provider() -> Result<SdkTracerProvider, DiagnosticsError> {
    let client = reqwest_otel::blocking::Client::builder()
        .timeout(EXPORT_TIMEOUT)
        .build()
        .map_err(|error| {
            DiagnosticsError::new(format!("failed to create OTLP HTTP client: {error}"))
        })?;
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_http_client(client)
        .with_protocol(Protocol::HttpBinary)
        .with_timeout(EXPORT_TIMEOUT)
        .build()
        .map_err(|error| {
            DiagnosticsError::new(format!("failed to create OTLP exporter: {error}"))
        })?;
    let resource = Resource::builder()
        .with_service_name("redcrown-desktop")
        .build();
    Ok(SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build())
}

fn otlp_enabled() -> bool {
    std::env::var(ENABLE_VARIABLE)
        .is_ok_and(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true"))
}

/// Reports diagnostics initialization failures.
#[derive(Debug)]
pub struct DiagnosticsError {
    message: String,
    backtrace: Backtrace,
}

impl DiagnosticsError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            backtrace: Backtrace::capture(),
        }
    }

    /// Returns the bounded message safe for local diagnostics.
    #[must_use]
    pub fn user_message(&self) -> &str {
        &self.message
    }
}

impl Display for DiagnosticsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}\n{}", self.message, self.backtrace)
    }
}

impl std::error::Error for DiagnosticsError {}

#[cfg(test)]
mod tests {
    use super::SAFE_TELEMETRY_TARGET;

    #[test]
    fn safe_target_is_separate_from_normal_module_events() {
        assert_eq!(SAFE_TELEMETRY_TARGET, "redcrown_telemetry");
        assert_ne!(SAFE_TELEMETRY_TARGET, module_path!());
    }
}
