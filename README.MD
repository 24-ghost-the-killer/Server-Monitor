# Server Monitor - NetPulse Engine

A high-performance, asynchronous network monitoring and infrastructure health check system built with Rust and Tokio. Monitor multiple servers and services across your infrastructure with real-time status tracking, comprehensive health checks, and an intuitive web dashboard.

## Live Preview

A public instance of the dashboard is available for preview:
> **Note:** If the server permits image linking, a live snapshot can be viewed directly below:
>
> ![Dashboard Preview](http://5.175.246.32:3000/)

(This link provides a quick way to see the dashboard without running the service locally.)

## Features

- **Multi-Protocol Health Checks**: Support for ICMP Ping, TCP port connectivity, and UDP port monitoring
- **Real-Time Monitoring**: Continuous infrastructure status tracking with configurable intervals
- **High Concurrency**: Handle up to 1500+ concurrent check operations (configurable)
- **Web Dashboard**: Beautiful real-time visualization of server status and health metrics
- **RESTful API**: Query current monitoring state via `/api/stats` endpoint
- **Organized Monitoring**: Group servers into categories for logical organization
- **Retry Logic**: Configurable retry mechanisms for transient failures
- **DNS Resolution**: Built-in DNS resolution with Cloudflare nameservers
- **Performance Metrics**: Track latency, packet loss, and check timestamps
- **Webhook Integration**: Optional webhook notifications for status changes (configurable)

## Architecture

### Core Components

1. **Monitor Engine** (`engine.rs`)
   - Manages the monitoring lifecycle
   - Handles concurrent check execution
   - Maintains infrastructure state
   - Tracks check results and metrics

2. **Configuration System** (`config.rs`)
   - JSON-based configuration
   - Support for server categories
   - Flexible check type definitions
   - Customizable timeouts and retry policies

3. **API Server** (`api.rs`)
   - Serves the web dashboard
   - Provides JSON REST endpoints
   - Real-time state exposure
   - Static file serving

4. **Models** (`models.rs`)
   - Type-safe data structures
   - Check result tracking
   - Status enumerations

## Getting Started

### Prerequisites

- Rust 1.70+ (edition 2024)
- Tokio async runtime
- Network connectivity to monitored targets

### Installation

1. Clone the repository:
```bash
git clone <repository-url>
cd "Server Monitor"
```

2. Ensure Rust is installed:
```bash
rustup update
```

3. Build the project:
```bash
cargo build --release
```

### Configuration

Create a `config.json` file in the project root with your monitoring configuration:

```json
{
    "check_interval": 20,
    "max_concurrency": 500,
    "api_port": 3000,
    "categories": [
        {
            "name": "Production Servers",
            "servers": [
                {
                    "name": "Web Server 1",
                    "address": "192.168.1.100",
                    "max_retries": 2,
                    "checks": [
                        {
                            "type": "Ping",
                            "timeout_ms": 3500
                        },
                        {
                            "type": "TcpPort",
                            "port": 80,
                            "timeout_ms": 1000
                        },
                        {
                            "type": "TcpPort",
                            "port": 443,
                            "timeout_ms": 1000
                        }
                    ]
                }
            ]
        }
    ]
}
```

### Configuration Options

#### Root Configuration

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `check_interval` | u64 | - | Interval between checks (seconds) |
| `max_concurrency` | usize | 1500 | Maximum concurrent check operations |
| `api_port` | u16 | 3000 | HTTP server port |
| `webhook_url` | string (optional) | - | URL for status change webhooks |

#### Server Configuration

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `name` | string | - | Server display name |
| `address` | string | - | IP address or hostname |
| `max_retries` | u32 | 1 | Retry attempts on failure |
| `checks` | array | - | Array of health checks |

#### Check Types

**Ping Check**
```json
{
    "type": "Ping",
    "count": 1,
    "timeout_ms": 3500
}
```

**TCP Port Check**
```json
{
    "type": "TcpPort",
    "port": 8080,
    "timeout_ms": 1000
}
```

**UDP Port Check**
```json
{
    "type": "UdpPort",
    "port": 53,
    "timeout_ms": 1000
}
```

### Running the Application

```bash
cargo run --release
```

The application will:
1. Load configuration from `config.json`
2. Initialize the monitoring engine
3. Start the web server (default: http://localhost:3000)
4. Begin continuous health checks

### Web Dashboard

Access the real-time monitoring dashboard at:
```
http://localhost:3000
```

The dashboard displays:
- Server status (Up/Down)
- Latency measurements
- Packet loss statistics
- Last check timestamp
- Check categories and organization

### API Endpoints

#### Get Current Status
```
GET /api/stats
```

Returns JSON array of all check results:
```json
[
    {
        "category": "Mine Servers",
        "server_name": "A-Zone Server",
        "target_address": "5.175.246.32",
        "timestamp": "2026-02-19T10:30:45Z",
        "check_type": "TCP:22",
        "status": true,
        "latency_ms": 45.2,
        "packet_loss": null,
        "message": "Connection successful"
    }
]
```

## Project Structure

```
Server Monitor/
├── Cargo.toml              # Project dependencies
├── config.json             # Monitoring configuration
├── README.md               # This file
├── src/
│   ├── main.rs             # Application entry point
│   ├── engine.rs           # Core monitoring engine
│   ├── api.rs              # Web server and API handlers
│   ├── config.rs           # Configuration structures
│   ├── models.rs           # Data models
│   └── utils.rs            # Utility functions
└── public/
    ├── index.html          # Dashboard HTML
    ├── script.js           # Dashboard JavaScript
    └── style.css           # Dashboard styles
```

## Performance

The monitoring engine is designed for high-performance concurrent operations:

- **Concurrency Limiter**: Semaphore-based rate limiting prevents resource exhaustion
- **Async Runtime**: Tokio-based async execution for thousands of concurrent checks
- **Connection Pooling**: HTTP client with connection reuse via reqwest
- **DNS Caching**: Cloudflare DNS resolver for fast hostname resolution
- **Non-blocking**: All I/O operations are fully asynchronous

### Recommended Settings

| Infrastructure Size | check_interval | max_concurrency |
|---------------------|----------------|-----------------|
| Small (< 50 checks) | 30-60s | 50-100 |
| Medium (50-500 checks) | 20-30s | 200-500 |
| Large (500+ checks) | 15-20s | 500-1500 |

## Logging

The application uses the `tracing` crate for structured logging:

```bash
# Run with info logging (default)
cargo run --release

# Run with debug logging
RUST_LOG=debug cargo run --release

# Run with trace logging
RUST_LOG=trace cargo run --release
```

## Dependencies

Key dependencies include:
- **tokio**: Async runtime
- **surge-ping**: ICMP ping implementation
- **axum**: Web framework
- **serde/serde_json**: Serialization
- **chrono**: Timestamp handling
- **hickory-resolver**: DNS resolution
- **reqwest**: HTTP client
- **tower-http**: HTTP middleware

See [Cargo.toml](Cargo.toml) for the complete dependency list.

## Troubleshooting

### Port Already in Use
If the API port is already in use, change the `api_port` in `config.json`:
```json
{
    "api_port": 3001
}
```

### Permission Denied (ICMP)
On some systems, ICMP (ping) requires elevated privileges. Run with sudo or use only TCP/UDP checks.

### DNS Resolution Fails
Check internet connectivity and ensure Cloudflare nameservers are accessible (1.1.1.1, 1.0.0.1).

### High CPU Usage
Reduce `max_concurrency` or increase `check_interval` to lower the load.

## Contributing

Contributions are welcome! Please ensure:
- Code follows Rust conventions
- All tests pass: `cargo test`
- Code is formatted: `cargo fmt`
- No clippy warnings: `cargo clippy`

## License

This project is provided as-is for infrastructure monitoring purposes.

## Support

For issues, questions, or feature requests, please check the project documentation or create an issue in the repository.
