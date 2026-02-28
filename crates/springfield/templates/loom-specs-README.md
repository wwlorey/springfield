# Loom Specifications

Design documentation for Loom, an AI-powered coding agent in Rust.

## Core Architecture

| Spec | Code | Purpose |
|------|------|---------|
| [architecture.md](./architecture.md) | [crates/](../crates/) | Crate structure, server-side LLM proxy design |
| [state-machine.md](./state-machine.md) | [loom-core](../crates/loom-core/) | Agent state machine for conversation flow |
| [tool-system.md](./tool-system.md) | [loom-tools](../crates/loom-tools/) | Tool registry and execution framework |
| [thread-system.md](./thread-system.md) | [loom-thread](../crates/loom-thread/) | Thread persistence and sync |
| [streaming.md](./streaming.md) | [loom-llm-service](../crates/loom-llm-service/) | SSE streaming for real-time LLM responses |
| [error-handling.md](./error-handling.md) | [loom-core](../crates/loom-core/) | Error types using `thiserror` |

## Observability Suite

Loom's integrated observability platform: analytics, crash tracking, cron monitoring, and session health.

| Spec | Code | Purpose |
|------|------|---------|
| [analytics-system.md](./analytics-system.md) | [loom-analytics-core](../crates/loom-analytics-core/), [loom-analytics](../crates/loom-analytics/), [loom-server-analytics](../crates/loom-server-analytics/) | Product analytics with PostHog-style identity resolution |
| [crash-system.md](./crash-system.md) | [loom-crash-core](../crates/loom-crash-core/), [loom-crash](../crates/loom-crash/), [loom-crash-symbolicate](../crates/loom-crash-symbolicate/), [loom-server-crash](../crates/loom-server-crash/) | Crash analytics with source maps, regression detection |
| [sessions-system.md](./sessions-system.md) | [loom-sessions-core](../crates/loom-sessions-core/), [loom-server-sessions](../crates/loom-server-sessions/) | Session analytics with release health and crash-free rate |

## LLM Integration

| Spec | Code | Purpose |
|------|------|---------|
| [llm-client.md](./llm-client.md) | [loom-llm-anthropic](../crates/loom-llm-anthropic/), [loom-llm-openai](../crates/loom-llm-openai/), [loom-server-llm-zai](../crates/loom-server-llm-zai/) | `LlmClient` trait for providers |
| [anthropic-oauth-pool.md](./anthropic-oauth-pool.md) | [loom-llm-anthropic](../crates/loom-llm-anthropic/) | Claude subscription pooling with failover |
| [claude-subscription-auth.md](./claude-subscription-auth.md) | [loom-llm-anthropic](../crates/loom-llm-anthropic/) | OAuth 2.0 PKCE for Claude Pro/Max |
