# RFC 0006: DNS Management

- **Status:** Accepted
- **Author(s):** @ap-1
- **Created:** 2026-02-25
- **Updated:** 2026-02-25

## Overview

Automatically manage DNS records via provider APIs when deployments are created, updated, or torn down, using a trait-based architecture with initial Cloudflare implementation.

## Motivation

Currently, DNS records must be manually configured for each deployment. When a branch is deployed to `api-feature-x.myproject.scottylabs.org`, an administrator must manually create the DNS A/AAAA record pointing to the Kennel server.

This manual process:

- Slows down deployments (DNS setup is a bottleneck)
- Requires administrative access to DNS provider
- Is error-prone (typos, wrong IP addresses, forgotten cleanup)
- Doesn't scale with automated branch deployments

Automatic DNS management enables:

- Zero-touch deployment (push code, DNS appears automatically)
- Automatic cleanup when deployments are torn down
- Consistent DNS configuration across all deployments
- Support for both auto-generated subdomains and custom domains

## Goals

- Define generic DNS provider trait for extensibility
- Implement Cloudflare provider as reference implementation
- Automatically create DNS records when deployments become active
- Automatically delete DNS records when deployments are torn down
- Support both A (IPv4) and AAAA (IPv6) records
- Support wildcard DNS for base domain (`*.myproject.scottylabs.org`)
- Handle custom domains specified in kennel.toml
- Idempotent operations (safe to retry)

## Non-Goals

- Multiple DNS provider implementations (only Cloudflare initially, others can be added later)
- DNSSEC management
- Advanced DNS features (CNAME flattening, load balancing, geo-routing)
- Dynamic DNS updates based on server IP changes
- DNS record validation or health checks

## Detailed Design

### Architecture

The `kennel-dns` crate provides a provider-agnostic DNS management interface via traits. Provider-specific implementations handle API communication.

```
Deployer -> DnsManager -> DnsProvider trait -> CloudflareProvider
                              |
                              v
                         Database (track DNS records)
```

### DNS Provider Trait

```rust
#[async_trait]
pub trait DnsProvider: Send + Sync {
    async fn create_record(
        &self,
        name: &str,
        record_type: RecordType,
        content: &str,
    ) -> Result<DnsRecord>;

    async fn delete_record(&self, provider_record_id: &str) -> Result<()>;

    async fn list_records(&self) -> Result<Vec<DnsRecord>>;
}

#[derive(Debug, Clone)]
pub struct DnsRecord {
    pub provider_record_id: String,
    pub name: String,
    pub record_type: RecordType,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordType {
    A,
    AAAA,
}
```

This trait abstracts DNS operations across providers. Any DNS provider can be implemented by providing these three operations.

### Cloudflare Provider Implementation

Use the official [cloudflare crate](https://crates.io/crates/cloudflare) for Cloudflare API v4 integration.

```rust
use cloudflare::endpoints::dns;
use cloudflare::framework::{auth::Credentials, HttpApiClient, HttpApiClientConfig};

pub struct CloudflareProvider {
    client: HttpApiClient,
    zone_identifier: String,
}

impl CloudflareProvider {
    pub fn new(api_token: String, zone_id: String) -> Result<Self> {
        let credentials = Credentials::UserAuthToken { token: api_token };
        let client = HttpApiClient::new(
            credentials,
            HttpApiClientConfig::default(),
            Environment::Production,
        )?;
        
        Ok(Self {
            client,
            zone_identifier: zone_id,
        })
    }
}

#[async_trait]
impl DnsProvider for CloudflareProvider {
    async fn create_record(
        &self,
        name: &str,
        record_type: RecordType,
        content: &str,
    ) -> Result<DnsRecord> {
        use dns::{CreateDnsRecord, CreateDnsRecordParams, DnsContent};

        let dns_content = match record_type {
            RecordType::A => DnsContent::A { content: content.parse()? },
            RecordType::AAAA => DnsContent::AAAA { content: content.parse()? },
        };

        let params = CreateDnsRecordParams {
            name,
            content: dns_content,
            ttl: Some(300),
            proxied: Some(true),  // Enable Cloudflare proxy
            priority: None,
        };

        let response = self.client
            .request(&CreateDnsRecord {
                zone_identifier: &self.zone_identifier,
                params,
            })
            .await?;

        Ok(DnsRecord {
            provider_record_id: response.result.id,
            name: response.result.name,
            record_type,
            content: content.to_string(),
        })
    }

    async fn delete_record(&self, provider_record_id: &str) -> Result<()> {
        use dns::DeleteDnsRecord;

        self.client
            .request(&DeleteDnsRecord {
                zone_identifier: &self.zone_identifier,
                identifier: provider_record_id,
            })
            .await?;

        Ok(())
    }

    async fn list_records(&self) -> Result<Vec<DnsRecord>> {
        use dns::{ListDnsRecords, ListDnsRecordsParams};

        let response = self.client
            .request(&ListDnsRecords {
                zone_identifier: &self.zone_identifier,
                params: ListDnsRecordsParams::default(),
            })
            .await?;

        Ok(response.result.into_iter().filter_map(|r| {
            let (record_type, content) = match r.content {
                DnsContent::A { content } => (RecordType::A, content.to_string()),
                DnsContent::AAAA { content } => (RecordType::AAAA, content.to_string()),
                _ => return None,
            };

            Some(DnsRecord {
                provider_record_id: r.id,
                name: r.name,
                record_type,
                content,
            })
        }).collect())
    }
}
```

### DNS Manager

The DNS manager coordinates DNS operations across multiple zones and database tracking:

```rust
use std::collections::HashMap;

pub struct DnsManager {
    providers: HashMap<String, Arc<dyn DnsProvider>>,
    store: Arc<Store>,
    server_ipv4: Ipv4Addr,
    server_ipv6: Ipv6Addr,
}

impl DnsManager {
    pub fn new(
        providers: HashMap<String, Arc<dyn DnsProvider>>,
        store: Arc<Store>,
        server_ipv4: Ipv4Addr,
        server_ipv6: Ipv6Addr,
    ) -> Self {
        Self {
            providers,
            store,
            server_ipv4,
            server_ipv6,
        }
    }

    fn get_provider_for_domain(&self, domain: &str) -> Result<&Arc<dyn DnsProvider>> {
        for (zone, provider) in &self.providers {
            if domain.ends_with(zone) {
                return Ok(provider);
            }
        }
        Err(anyhow!("No DNS provider configured for domain: {}", domain))
    }

    pub async fn create_record_for_deployment(
        &self,
        deployment_id: i64,
        domain: &str,
    ) -> Result<()> {
        let provider = self.get_provider_for_domain(domain)?;

        // Always create both A and AAAA records
        let a_record = provider
            .create_record(domain, RecordType::A, &self.server_ipv4.to_string())
            .await?;
        
        self.store.dns_records()
            .create(domain, deployment_id, &a_record.provider_record_id, "A", &self.server_ipv4.to_string())
            .await?;

        let aaaa_record = provider
            .create_record(domain, RecordType::AAAA, &self.server_ipv6.to_string())
            .await?;
        
        self.store.dns_records()
            .create(domain, deployment_id, &aaaa_record.provider_record_id, "AAAA", &self.server_ipv6.to_string())
            .await?;

        Ok(())
    }

    pub async fn delete_record_for_deployment(&self, deployment_id: i64) -> Result<()> {
        let records = self.store.dns_records()
            .find_by_deployment(deployment_id)
            .await?;

        for record in records {
            let provider = self.get_provider_for_domain(&record.domain)?;
            provider.delete_record(&record.provider_record_id).await?;
            self.store.dns_records().delete(record.id).await?;
        }

        Ok(())
    }

    pub async fn reconcile(&self) -> Result<ReconciliationSummary> {
        // Reconciliation logic
    }
}
```

The `DnsManager` accepts any implementation of `DnsProvider` and supports multiple zones by mapping domains to their appropriate provider instance.

### Database Schema

New table `dns_records`:

```sql
CREATE TABLE dns_records (
    id SERIAL PRIMARY KEY,
    domain TEXT NOT NULL UNIQUE,
    deployment_id INTEGER REFERENCES deployments(id) ON DELETE CASCADE,
    provider_record_id TEXT NOT NULL,
    record_type TEXT NOT NULL,  -- 'A' or 'AAAA'
    ip_address TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_dns_records_deployment_id ON dns_records(deployment_id);
CREATE INDEX idx_dns_records_domain ON dns_records(domain);
```

Fields:

- `domain` - Fully qualified domain name (e.g., `api-main.myproject.scottylabs.org`)
- `deployment_id` - Foreign key to deployment (NULL for wildcard/base domain records)
- `provider_record_id` - Provider's record ID for deletion (provider-specific format)
- `record_type` - 'A' or 'AAAA'
- `ip_address` - Server IP address

Add `dns_status` to `deployments` table:

```sql
ALTER TABLE deployments ADD COLUMN dns_status TEXT DEFAULT 'pending';
-- Values: 'pending', 'active', 'failed'
```

### DNS Record Lifecycle

**On deployment creation:**

1. Deployment becomes active (health check passes)
1. Deployer calls `dns_manager.create_record_for_deployment(deployment_id, domain)`
1. DNS manager creates record via provider trait
1. Record stored in `dns_records` table with `provider_record_id`
1. Deployment `dns_status` updated to 'active'

**On deployment teardown:**

1. Deployer receives teardown request
1. Deployer calls `dns_manager.delete_record_for_deployment(deployment_id)`
1. DNS manager deletes records via provider trait
1. Records removed from `dns_records` table

**On Kennel startup:**

1. Reconciliation finds deployments with `dns_status = 'pending'`
1. Retry DNS record creation
1. Find orphaned DNS records (no matching deployment)
1. Delete orphaned records

### Integration with Deployer

Deployer calls DNS manager after successful deployment:

```rust
// In deployer/src/service.rs
async fn deploy_service(req: DeploymentRequest, config: &DeployerConfig) -> Result<()> {
    // ... existing deployment logic ...

    // Mark deployment as active
    store.deployments().update_status(deployment_id, DeploymentStatus::Active).await?;

    // Create DNS records
    if let Some(dns_manager) = &config.dns_manager {
        match dns_manager.create_record_for_deployment(deployment_id, &domain).await {
            Ok(_) => {
                store.deployments().update_dns_status(deployment_id, "active").await?;
            }
            Err(e) => {
                tracing::error!("DNS creation failed: {}", e);
                store.deployments().update_dns_status(deployment_id, "failed").await?;
            }
        }
    }

    // Notify router
    router_tx.send(RouterUpdate::DeploymentActivated { ... }).await?;

    Ok(())
}
```

Teardown deletes DNS records:

```rust
// In deployer/src/teardown.rs
async fn teardown_deployment(deployment_id: i64, config: &DeployerConfig) -> Result<()> {
    // Delete DNS records first
    if let Some(dns_manager) = &config.dns_manager {
        if let Err(e) = dns_manager.delete_record_for_deployment(deployment_id).await {
            tracing::error!("DNS deletion failed: {}", e);
        }
    }

    // ... existing teardown logic ...

    Ok(())
}
```

### Wildcard DNS per Project

Create wildcard DNS records per project to avoid conflicts with other services on the base domain:

```
*.myproject.scottylabs.org -> 1.2.3.4
```

This allows all branch deployments for a project to resolve automatically. When a project is created, a wildcard DNS record is created for that project's subdomain.

**Wildcard Setup:**

When a project is added to Kennel configuration, create its wildcard record:

```rust
// For project "myproject" on base domain "scottylabs.org"
dns_manager.create_wildcard_record(&format!("*.{}.{}", project_name, base_domain)).await?;
```

Stored in `dns_records` with `deployment_id = NULL` and linked to project.

### Configuration

Environment variables:

```bash
# DNS Provider
DNS_PROVIDER=cloudflare  # Currently only 'cloudflare' supported

# Cloudflare-specific (supports multiple zones as comma-separated)
CLOUDFLARE_API_TOKEN=your-token
CLOUDFLARE_ZONES=scottylabs.org:zone-id-1,example.com:zone-id-2

# Server IP addresses
SERVER_IPV4=1.2.3.4
SERVER_IPV6=2001:db8::1
```

NixOS module options:

```nix
services.kennel.dns = {
  enable = mkEnableOption "Automatic DNS management";

  provider = mkOption {
    type = types.enum [ "cloudflare" ];
    default = "cloudflare";
    description = "DNS provider to use";
  };

  cloudflare = {
    apiTokenFile = mkOption {
      type = types.path;
      example = "/run/secrets/cloudflare-api-token";
      description = "Path to file containing Cloudflare API token";
    };

    zones = mkOption {
      type = types.attrsOf types.str;
      example = {
        "scottylabs.org" = "abc123def456";
        "example.com" = "xyz789ghi012";
      };
      description = "Map of domain names to Cloudflare zone IDs";
    };
  };

  serverIpv4 = mkOption {
    type = types.str;
    example = "1.2.3.4";
    description = "Server IPv4 address for DNS records";
  };

  serverIpv6 = mkOption {
    type = types.str;
    example = "2001:db8::1";
    description = "Server IPv6 address for DNS records";
  };
};
```

### Provider Initialization

In `main.rs`, initialize providers for each configured zone:

```rust
let dns_manager = if config.dns.enable {
    let mut providers = HashMap::new();
    
    match config.dns.provider.as_str() {
        "cloudflare" => {
            for (domain, zone_id) in &config.dns.cloudflare_zones {
                let provider = Arc::new(CloudflareProvider::new(
                    config.dns.cloudflare_api_token.clone(),
                    zone_id.clone(),
                )?) as Arc<dyn DnsProvider>;
                providers.insert(domain.clone(), provider);
            }
        }
        _ => return Err(anyhow!("Unsupported DNS provider")),
    };

    Some(Arc::new(DnsManager::new(
        providers,
        store.clone(),
        config.dns.server_ipv4,
        config.dns.server_ipv6,
    )))
} else {
    None
};
```

### Error Handling

DNS operations are non-critical - deployment can succeed even if DNS fails.

**Strategy:**

- Log DNS errors but don't fail deployment
- Retry with exponential backoff (3 attempts)
- Reconciliation will fix missing DNS records on next startup
- Track DNS status separately in `deployments.dns_status`

### Reconciliation

On startup, reconcile DNS records:

```rust
pub async fn reconcile(&self) -> Result<ReconciliationSummary> {
    let mut summary = ReconciliationSummary::default();

    // Find deployments with pending DNS
    let deployments = self.store.deployments()
        .find_by_dns_status("pending")
        .await?;

    for deployment in deployments {
        match self.create_record_for_deployment(deployment.id, &deployment.domain).await {
            Ok(_) => {
                self.store.deployments()
                    .update_dns_status(deployment.id, "active")
                    .await?;
                summary.dns_created += 1;
            }
            Err(e) => {
                tracing::error!("Failed to create DNS for deployment {}: {}", deployment.id, e);
                summary.dns_failed += 1;
            }
        }
    }

    // Find orphaned DNS records (no matching deployment)
    let provider_records = self.provider.list_records().await?;
    let our_records = self.store.dns_records().find_all().await?;

    for provider_record in provider_records {
        if !our_records.iter().any(|r| r.provider_record_id == provider_record.provider_record_id) {
            tracing::warn!("Found orphaned DNS record: {}, deleting", provider_record.name);
            self.provider.delete_record(&provider_record.provider_record_id).await?;
            summary.dns_orphaned += 1;
        }
    }

    Ok(summary)
}
```

### Custom Domains

Custom domains from kennel.toml are handled identically:

```toml
[[static_sites]]
name = "web"
output = ".#kennelWeb"
custom_domain = "kennel.scottylabs.org"
spa = true
```

When deployment is created:

1. DNS record created for `web-main.myproject.scottylabs.org` (auto-generated)
1. DNS record created for `kennel.scottylabs.org` (custom domain)

Both point to the same server IP. Router handles routing based on Host header.

## Alternatives Considered

### Trait-based vs Provider-specific Implementation

**Chosen:** Trait-based with generic `DnsProvider` interface

**Alternative:** Hardcode Cloudflare API directly in `kennel-dns`

**Reasoning:** Trait-based design allows adding Route 53, Google Cloud DNS, or other providers later without changing core logic. Minimal overhead, standard Rust pattern for abstraction.

### DNS Provider: Cloudflare vs Others

**Chosen:** Cloudflare as initial implementation

**Alternatives:** Route 53 (AWS), Google Cloud DNS, Azure DNS, self-hosted DNS (BIND, PowerDNS)

**Reasoning:** Cloudflare has a simple REST API, generous free tier, fast global propagation, and is already used by ScottyLabs. Trait design allows adding other providers later.

### Wildcard DNS vs Individual Records

**Chosen:** Wildcard DNS for base domain, individual records for custom domains

**Alternative:** Create individual DNS records for every deployment

**Reasoning:** Wildcard DNS reduces API calls and provider record limits. Individual records are only needed for custom domains outside the base domain pattern.

### DNS Update Timing: Immediate vs Batched

**Chosen:** Immediate updates on deployment activation/teardown

**Alternative:** Batch DNS updates every N minutes

**Reasoning:** Immediate updates provide faster deployment experience. DNS propagation already has inherent delay, so batching doesn't improve perceived latency.

### Error Handling: Fail Deployment vs Retry Later

**Chosen:** Don't fail deployment, retry during reconciliation

**Alternative:** Fail deployment if DNS creation fails

**Reasoning:** DNS is not critical for internal access (can use IP or port directly). Deployments should succeed even if DNS temporarily fails. Reconciliation ensures eventual consistency.

## Open Questions

1. **DNS TTL:** What TTL should be used for deployment DNS records? Low TTL (60s) for faster updates vs higher TTL (300s) for better caching?

1. **Rate limiting:** DNS provider APIs have rate limits. Should we implement client-side rate limiting or rely on provider errors?

1. **DNS validation:** Should we validate DNS propagation before marking deployment as active? Or trust that provider API success means DNS is valid?

## Implementation Phases

### Trait Definition and Core Types

Define `DnsProvider` trait with create, delete, and list operations. Define `DnsRecord` and `RecordType` types. Create error types for DNS operations.

### Database Schema

Create migration for `dns_records` table. Add `dns_status` column to `deployments` table. Generate SeaORM entities for `dns_records`. Create repository methods for DNS record CRUD.

### Cloudflare Provider

Implement `CloudflareProvider` struct with API token and zone ID. Implement `DnsProvider` trait for Cloudflare. Handle Cloudflare API authentication and request/response formats. Add retry logic with exponential backoff. Write unit tests with mock HTTP server.

### DNS Manager

Implement `DnsManager` with provider-agnostic interface. Add deployment record lifecycle operations. Integrate with kennel-store for database tracking. Implement reconciliation logic. Write integration tests with mock provider.

### Deployer Integration

Add DNS manager to deployer configuration. Call DNS operations on deployment activation and teardown. Handle DNS errors gracefully without failing deployments. Update deployment `dns_status` field.

### NixOS Module Integration

Add `services.kennel.dns.*` options to NixOS module. Configure environment variables from NixOS options. Use `EnvironmentFile` for API token security. Add assertions for required DNS configuration. Initialize provider in main.rs based on configuration.

### Wildcard DNS Setup

Implement wildcard DNS creation on startup. Store wildcard records in database with `deployment_id = NULL`. Add reconciliation to ensure wildcard exists.

### Documentation

Document DNS provider trait for implementing new providers. Add Cloudflare API setup and token creation guide. Update NixOS deployment guide with DNS configuration. Document troubleshooting for DNS issues.
