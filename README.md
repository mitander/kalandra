# kalandra

A high-assurance messaging protocol combining **End-to-End Encryption** with **Server-Side Moderation**.

Built on [MLS](https://www.rfc-editor.org/rfc/rfc9420.pdf) and [QUIC](https://www.rfc-editor.org/rfc/rfc9000.pdf) which allows servers to cryptographically enforce bans, ordering, and group membership without accessing message content.

Prioritizes correctness through **Deterministic Simulation Testing** and an **action-based architecture**.

## Design

- **Hub-Centric:** Servers enforce total ordering and moderation via MLS External Commits.
- **Action-Based:** Protocol logic returns actions for the driver to execute, keeping it pure and testable.
- **Zero-Copy:** Wire format designed for O(1) routing.

## Workspace

- `crates/kalandra-core`: Pure protocol state machines (Connection, MLS).
- `crates/kalandra-proto`: Wire format definitions and serialization.
- `crates/kalandra-harness`: Deterministic simulation runner (using `turmoil`).
- `crates/kalandra-server`: Production server runtime (using `quinn` + `tokio`).

### Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Protocol Specification](docs/PROTOCOL.md)
- [Implementation Roadmap](IMPLEMENTATION_PLAN.md)

## License

[Apache 2.0](LICENSE)
