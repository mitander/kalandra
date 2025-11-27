//! Determinism tests for scenario framework.
//!
//! Verifies that scenarios produce identical results across multiple runs.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use sunder_core::connection::ConnectionState;
use sunder_harness::scenario::Scenario;

/// Captured state from a scenario run
#[derive(Debug, Clone, PartialEq, Eq)]
struct ScenarioState {
    client_state: ConnectionState,
    server_state: ConnectionState,
    client_frames_sent: usize,
    server_frames_sent: usize,
    client_frames_received: usize,
    server_frames_received: usize,
}

#[test]
fn scenario_determinism_handshake() {
    // Run the same scenario 10 times and capture state
    let mut states = Vec::new();

    for _ in 0..10 {
        let captured_state = Arc::new(Mutex::new(None));
        let captured_state_clone = Arc::clone(&captured_state);

        let result = Scenario::new()
            .oracle(Box::new(move |world| {
                *captured_state_clone.lock().unwrap() = Some(ScenarioState {
                    client_state: world.client().state(),
                    server_state: world.server().state(),
                    client_frames_sent: world.client_frames_sent(),
                    server_frames_sent: world.server_frames_sent(),
                    client_frames_received: world.client_frames_received(),
                    server_frames_received: world.server_frames_received(),
                });
                Ok(())
            }))
            .run();

        assert!(result.is_ok(), "Scenario should succeed");
        let state =
            captured_state.lock().unwrap().clone().expect("Oracle should have captured state");
        states.push(state);
    }

    // All runs should produce identical results
    let first = &states[0];
    for (i, state) in states.iter().enumerate().skip(1) {
        assert_eq!(state, first, "Run {} produced different results than run 0", i);
    }
}

#[test]
fn scenario_determinism_with_time_advance() {
    // Run scenario with time advancement multiple times
    let mut states = Vec::new();

    for _ in 0..10 {
        let captured_state = Arc::new(Mutex::new(None));
        let captured_state_clone = Arc::clone(&captured_state);

        let result = Scenario::new()
            .with_time_advance(Duration::from_secs(45))
            .oracle(Box::new(move |world| {
                *captured_state_clone.lock().unwrap() = Some(ScenarioState {
                    client_state: world.client().state(),
                    server_state: world.server().state(),
                    client_frames_sent: world.client_frames_sent(),
                    server_frames_sent: world.server_frames_sent(),
                    client_frames_received: world.client_frames_received(),
                    server_frames_received: world.server_frames_received(),
                });
                Ok(())
            }))
            .run();

        assert!(result.is_ok(), "Scenario should succeed");
        let state =
            captured_state.lock().unwrap().clone().expect("Oracle should have captured state");
        states.push(state);
    }

    // All runs should produce identical results
    let first = &states[0];
    for (i, state) in states.iter().enumerate().skip(1) {
        assert_eq!(state, first, "Run {} produced different results than run 0", i);
    }

    // Verify heartbeats were sent (sanity check)
    assert_eq!(first.client_state, ConnectionState::Authenticated);
    assert_eq!(first.server_state, ConnectionState::Authenticated);
    assert!(first.client_frames_sent > 1, "Client should have sent heartbeats");
    assert!(first.server_frames_sent > 1, "Server should have sent heartbeats");
}

#[test]
fn scenario_determinism_timeout() {
    // Run timeout scenario multiple times
    let mut states = Vec::new();

    for _ in 0..10 {
        let captured_state = Arc::new(Mutex::new(None));
        let captured_state_clone = Arc::clone(&captured_state);

        let result = Scenario::new()
            .with_time_advance(Duration::from_secs(61))
            .oracle(Box::new(move |world| {
                *captured_state_clone.lock().unwrap() = Some(ScenarioState {
                    client_state: world.client().state(),
                    server_state: world.server().state(),
                    client_frames_sent: world.client_frames_sent(),
                    server_frames_sent: world.server_frames_sent(),
                    client_frames_received: world.client_frames_received(),
                    server_frames_received: world.server_frames_received(),
                });
                Ok(())
            }))
            .run();

        assert!(result.is_ok(), "Scenario should succeed");
        let state =
            captured_state.lock().unwrap().clone().expect("Oracle should have captured state");
        states.push(state);
    }

    // All runs should produce identical results
    let first = &states[0];
    for (i, state) in states.iter().enumerate().skip(1) {
        assert_eq!(state, first, "Run {} produced different results than run 0", i);
    }

    // Both should be closed due to timeout
    assert_eq!(first.client_state, ConnectionState::Closed);
    assert_eq!(first.server_state, ConnectionState::Closed);
}
