# RFC 0013: Observability

- **Status:** Accepted
- **Author(s):** @ap-1
- **Created:** 2026-03-12
- **Updated:** 2026-03-12

## Overview

Add structured tracing and Prometheus metrics to Kennel. Tracing provides correlated spans across the webhook-build-deploy-route pipeline with OpenTelemetry export. Metrics expose operational counters, gauges, and histograms for monitoring and alerting.

## Motivation

Kennel has no observability infrastructure. Log messages are unstructured and uncorrelated -- debugging a failed deployment requires manually grepping for build IDs, deployment IDs, and DNS record IDs across separate log lines. There are no metrics for build rates, deployment health, proxy latency, or queue depth.

Operators need:

- **Tracing**: a trace ID that threads from webhook receipt through build, deploy, DNS creation, and routing. When something fails, follow the trace.
- **Metrics**: counters, histograms, and gauges feeding dashboards and alerting rules. When something degrades, see it immediately.

## Goals

- Structured tracing spans on all major operations with correlated trace IDs
- OpenTelemetry export via OTLP for Jaeger/Grafana Tempo
- Prometheus metrics endpoint on the API server
- Key metrics covering builds, deployments, proxy, health checks, and queue state
- NixOS module integration for OTLP endpoint and Prometheus scrape configuration

## Non-Goals

- Building dashboards (Grafana configuration, not Kennel code)
- Application-level metrics for deployed services
- Log aggregation infrastructure

## Detailed Design

### Tracing

Kennel already depends on `tracing` for logging. The change is adding `#[instrument]` attributes to key functions, propagating trace context through the pipeline, and exporting spans via OpenTelemetry.

#### Span Structure

The top-level span is created at the webhook handler. Every downstream operation inherits this context.

```
webhook_received [project, event_type, source_ip]
  create_build [build_id, project, branch, commit_sha]
    process_build [build_id]
      git_clone [build_id, repo_url, branch]
      evaluate_devenv [build_id]
      nix_build [build_id, service_name]
        cachix_push [build_id, service_name, cache]
      finalize_build [build_id, status]
    deploy_build [build_id]
      deploy_service [build_id, deployment_id, service_name, environment]
        provision_resource [deployment_id, provider, resource_name]
        resolve_secrets [deployment_id, profile]
        supervisor_start [deployment_id, process_name]
          readiness_probe [process_name, attempt]
        create_dns [deployment_id, domain, record_type]
      deploy_static_site [build_id, site_name]
```

For teardown:

```
process_teardown [deployment_id, project, branch, service]
  supervisor_stop [process_name]
  teardown_resource [deployment_id, provider]
  delete_dns [deployment_id, domain]
  remove_user [username]
```

#### Trace ID Propagation

The webhook handler generates a trace ID and attaches it as a span field. This ID is stored on the build record so downstream stages can continue the trace:

- Webhook creates build with `trace_id` field
- Builder reads `trace_id`, creates a span with it
- Deployer reads `trace_id`, continues the trace

All log lines within the trace share the same `trace_id` field, enabling correlation across pipeline stages without full W3C Trace Context propagation.

#### OpenTelemetry Export

Spans are exported via OTLP using `tracing-opentelemetry` and `opentelemetry-otlp`. The OTLP endpoint is configured via the NixOS module. Structured JSON logs are emitted in parallel via `tracing-subscriber` for local debugging.

### Metrics

#### Library Choice

`metrics` crate (facade) with `metrics-exporter-prometheus` (backend). Mirrors the `tracing`/`tracing-subscriber` pattern.

#### Endpoint

`/metrics` on the existing API server. Prometheus scrapes on a configured interval.

#### Metric Definitions

**Builds:**

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `kennel_builds_total` | Counter | `project`, `status` | Builds by final status |
| `kennel_build_duration_seconds` | Histogram | `project` | Queued to terminal status |
| `kennel_builds_active` | Gauge | | Currently Building |
| `kennel_builds_queued` | Gauge | | Currently Queued |

**Deployments:**

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `kennel_deployments_total` | Counter | `project`, `environment` | Deployments created |
| `kennel_deployments_active` | Gauge | `environment` | Currently Deployed |
| `kennel_deployment_duration_seconds` | Histogram | `project` | Built to Deployed |
| `kennel_teardowns_total` | Counter | `project` | Teardowns completed |

**Proxy:**

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `kennel_proxy_requests_total` | Counter | `project`, `status_code` | Proxied requests |
| `kennel_proxy_request_duration_seconds` | Histogram | `project` | Request latency |
| `kennel_proxy_errors_total` | Counter | `project`, `error_type` | Proxy errors |

**Health checks:**

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `kennel_healthcheck_total` | Counter | `process`, `result` | Probe results |
| `kennel_processes_healthy` | Gauge | | Ready processes |
| `kennel_processes_unhealthy` | Gauge | | Unhealthy processes |

**DNS:**

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `kennel_dns_operations_total` | Counter | `operation`, `status` | Create/delete operations |

#### Instrumentation Points

- **Build counters**: builder worker, after status transitions
- **Deployment counters**: deployer, after creating/updating records
- **Proxy metrics**: proxy handler, wrapping the backend call
- **Health checks**: supervision task, after each probe
- **Gauges**: maintained as in-memory counters, updated on state transitions

### NixOS Module

```nix
services.kennel.observability = {
  metrics.enable = mkEnableOption "Prometheus metrics endpoint";
  tracing.otlpEndpoint = mkOption {
    type = types.str;
    example = "http://localhost:4317";
    description = "OTLP endpoint for trace export.";
  };
};
```

### Database Changes

Add a `trace_id` column (text, nullable) to the builds table for trace propagation across pipeline stages.

## Alternatives Considered

**OpenTelemetry for metrics too.** Use the OpenTelemetry SDK for both metrics and traces, exporting everything via OTLP. This provides a unified pipeline but ties metrics to an external collector. Prometheus pull-based scraping is more standard for infrastructure metrics and does not require a collector.

**Statsd/Graphite.** Push-based metrics. Less standard than Prometheus in the NixOS ecosystem.

## Open Questions

- **Gauge refresh**: should gauges (active builds, active deployments) be maintained as in-memory counters updated on state transitions, or computed via DB query on each Prometheus scrape? In-memory counters are faster but can drift after crashes; DB queries are accurate but add load per scrape.

## Implementation Phases

### Tracing

Add `#[instrument]` to key pipeline functions. Add `trace_id` to builds table. Configure `tracing-subscriber` with JSON output and `tracing-opentelemetry` with OTLP export. Add `opentelemetry-otlp` dependency.

### Prometheus Metrics

Add `metrics` and `metrics-exporter-prometheus`. Instrument build, deployment, and proxy paths. Expose `/metrics`. Update NixOS module.
