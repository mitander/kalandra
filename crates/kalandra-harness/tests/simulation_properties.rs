//! Property-based tests for simulation framework determinism
//!
//! These tests verify that the scenario/simulation framework produces
//! deterministic results across multiple runs with the same inputs.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use kalandra_core::connection::ConnectionState;
use kalandra_harness::scenario::Scenario;
use proptest::prelude::*;

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
fn prop_all_simulations_deterministic() {
    proptest!(|(
        time_advance_secs in 0u64..120,
    )| {
        let time_advance = Duration::from_secs(time_advance_secs);

        // Run the same scenario twice and verify identical results
        let mut states = Vec::new();

        for _ in 0..2 {
            let captured_state = Arc::new(Mutex::new(None));
            let captured_state_clone = Arc::clone(&captured_state);

            let result = Scenario::new()
                .with_time_advance(time_advance)
                .oracle(Box::new(move |world| {
                    *captured_state_clone.lock().expect("mutex poisoned") = Some(ScenarioState {
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

            prop_assert!(result.is_ok(), "Scenario should succeed");
            let state = captured_state
                .lock()
                .expect("mutex poisoned")
                .clone()
                .expect("Oracle should have captured state");
            states.push(state);
        }

        // PROPERTY: Determinism - same inputs produce same outputs
        prop_assert_eq!(
            &states[0],
            &states[1],
            "Same scenario with time_advance={:?} must produce identical results across runs",
            time_advance
        );
    });
}
