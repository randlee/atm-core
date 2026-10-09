use opentelemetry::trace::Tracer;

fn on_event(provider: &Provider) {
    let _ = provider.force_flush();
}
