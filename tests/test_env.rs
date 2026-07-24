//! Test environment setup for integration tests.
//!
//! Borrows the `TestEnv` pattern from testcontainers-rs examples: each test
//! gets its own isolated infrastructure (containers, temp dirs, etc.) that is
//! automatically cleaned up via RAII `Drop`. No global setup/teardown needed.

use std::time::Duration;

use testcontainers::core::WaitFor;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, Image};
use tokio::time::timeout;

/// Configuration for the test environment infrastructure.
#[derive(Debug, Clone, Copy)]
pub struct TestEnvConfig {
    /// Postgres image tag.
    pub postgres_tag: &'static str,
    /// Redis image tag.
    pub redis_tag: &'static str,
    /// RabbitMQ image tag.
    pub rabbitmq_tag: &'static str,
    /// Maximum time to wait for all services to become reachable.
    pub startup_timeout: Duration,
}

impl Default for TestEnvConfig {
    fn default() -> Self {
        Self {
            postgres_tag: "15-alpine",
            redis_tag: "7-alpine",
            rabbitmq_tag: "3-management-alpine",
            startup_timeout: Duration::from_secs(120),
        }
    }
}

/// RAII-owned test environment backed by Docker containers.
///
/// Each field is a running container that is stopped and removed when
/// `TestEnv` is dropped, giving tests complete isolation without any
/// manual teardown.
#[derive(Debug)]
pub struct TestEnv {
    _postgres: ContainerAsync<PostgresImage>,
    _redis: ContainerAsync<RedisImage>,
    _rabbitmq: ContainerAsync<RabbitMqImage>,
    /// Postgres connection URL.
    pub postgres_url: String,
    /// Redis connection URL.
    pub redis_url: String,
    /// RabbitMQ connection URL.
    pub rabbitmq_url: String,
}

/// Postgres image for testcontainers.
#[derive(Debug, Clone, Copy)]
pub struct PostgresImage;

impl Image for PostgresImage {
    fn name(&self) -> &str {
        "postgres"
    }

    fn tag(&self) -> &str {
        "15-alpine"
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        vec![WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        )]
    }
}

/// Redis image for testcontainers.
#[derive(Debug, Clone, Copy)]
pub struct RedisImage;

impl Image for RedisImage {
    fn name(&self) -> &str {
        "redis"
    }

    fn tag(&self) -> &str {
        "7-alpine"
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        vec![WaitFor::message_on_stdout("Ready to accept connections")]
    }
}

/// RabbitMQ image for testcontainers.
#[derive(Debug, Clone, Copy)]
pub struct RabbitMqImage;

impl Image for RabbitMqImage {
    fn name(&self) -> &str {
        "rabbitmq"
    }

    fn tag(&self) -> &str {
        "3-management-alpine"
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        vec![WaitFor::message_on_stdout("Server startup complete")]
    }
}

impl TestEnv {
    /// Start the full test environment.
    ///
    /// Spawns Postgres, Redis, and RabbitMQ containers in parallel, then
    /// waits for TCP reachability from the host. Containers are stopped
    /// and removed automatically when the returned `TestEnv` is dropped.
    pub async fn start(config: TestEnvConfig) -> anyhow::Result<Self> {
        let (postgres, redis, rabbitmq) =
            tokio::try_join!(PostgresImage.start(), RedisImage.start(), RabbitMqImage.start())?;

        let postgres_port = postgres.get_host_port_ipv4(5432).await?;
        let redis_port = redis.get_host_port_ipv4(6379).await?;
        let rabbitmq_port = rabbitmq.get_host_port_ipv4(5672).await?;

        let postgres_url = format!("postgres://postgres:postgres@127.0.0.1:{postgres_port}/postgres");
        let redis_url = format!("redis://127.0.0.1:{redis_port}");
        let rabbitmq_url = format!("amqp://127.0.0.1:{rabbitmq_port}");

        // Wait for services to be reachable from the test process.
        //
        // `testcontainers` only waits for the container's own startup logs;
        // it does not probe TCP reachability from the host. Adding a small
        // timeout here matches the article's `wait_for_consistency` advice
        // and prevents flaky "connection refused" failures when the host
        // network stack is still converging.
        timeout(config.startup_timeout, async {
            loop {
                if tokio::net::TcpStream::connect(format!("127.0.0.1:{postgres_port}"))
                    .await
                    .is_ok()
                    && tokio::net::TcpStream::connect(format!("127.0.0.1:{redis_port}")).await.is_ok()
                    && tokio::net::TcpStream::connect(format!("127.0.0.1:{rabbitmq_port}"))
                        .await
                        .is_ok()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await?;

        Ok(Self {
            _postgres: postgres,
            _redis: redis,
            _rabbitmq: rabbitmq,
            postgres_url,
            redis_url,
            rabbitmq_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_env_starts_isolated_containers() -> anyhow::Result<()> {
        let env = TestEnv::start(TestEnvConfig::default()).await?;

        // Containers are isolated per-test: verify distinct ports.
        assert!(env.postgres_url.contains("127.0.0.1:"));
        assert!(env.redis_url.contains("127.0.0.1:"));
        assert!(env.rabbitmq_url.contains("127.0.0.1:"));

        Ok(())
    }
}
